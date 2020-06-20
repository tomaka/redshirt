// Copyright (C) 2019-2020  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Timers handling on x86/x86_64.
//!
//! # Overview
//!
//! When it comes to monotonic time (as opposed to a real-life time clock), multiple mechanisms
//! exist:
//!
//! - The PIT (Programmable Interrupt Timer) is a legacy way of triggering an interrupt after a
//! certain amount of timer has elapsed. The PIT is unique on the system and is shared between
//! all the CPUs.
//! - x86/x86_64 CPUs optionally provide a register named TSC (TimeStamp Counter) whose value is
//! accessible using the `RDTSC` instruction. The value is increased at a uniform rate, even if
//! the processor is halted (`hlt` instruction). Each CPU on the system has a separate TSC value.
//! - Each CPU also has a local APIC that can trigger an interrupt after a certain number of
//! timer cycles has passed, or, if supported, when the TSC reaches a certain value. Each CPU has
//! its own local APIC, and the interrupt will only concern this CPU in particular.
//! - The HPET is a more recent version of the PIT. Like the PIT, there only exists at most one
//! HPET per machine.
//!
//! # Timers management
//!
//! ## One timer at a time
//!
//! In order to fire a single timer after a certain duration, we use the TSC as a reference point.
//! As part of the initialization process, we measure the rate at which the TSC increases, and
//! thus can determine at which TSC value the requested duration will have elapsed.
//!
//! We then use the local APIC in TSC deadline value mode if supported, or regular mode if not, to
//! fire an interrupt.
//!
//! In regular mode, considering that the timer value is 32bits, it might be necessary to chain
//! multiple timers before the desired TSC value is reached.
//!
//! By using the TSC as the reference value, we can check at any time whether the timer has been
//! fired. The local APIC's timer is use solely for its capability to wake up a halted CPU.
//!
//! Keep in mind that each CPU has its own TSC value, which is why we try to keep the TSCs of all
//! CPUs synchronized (see the [`../tsc_sync`] module). It is however possible that moving a TSC
//! value to a different CPU leads to this value being slightly superior to the current value of
//! the local TSC.
// TODO: ^ somehow enforce in the API that the TSC sync is indeed performed?
//!
//! ## Multiple timers
//!
//! In a perfect world we would like to either distribute timers uniformly amongst the multiple
//! CPUs, so that the overhead of handling interrupts is distributed uniformly, or alternatively
//! we would like to setup a timer directly on the CPU that is actually waiting for the timer to
//! be fired.
//!
//! In practice, though, we cannot directly configure the local APICs of other CPUs, and for the
//! sake of simplicity we employ the following strategy:
//!
//! - There exists a list of timers shared between all CPUs.
//! - When a timer is created:
//!   - If the current CPU is already handling a timer:
//!      - If the currently-handled timer will fire sooner than the newly-created timer, add the
//!        newly-created timer to this shared list.
//!      - If instead the currently-handled timer would fire later than the newly-created timer,
//!        add the current timer to this shared list and configure the current CPU for the
//!        newly-created timer.
//!   - If the current CPU is not currently handling any timer, configure the current CPU for the
//!        newly-created timer.
//! - When a timer interrupt is fired, the CPU that has been interrupted picks the next pending
//!   timer from the shared list and configures itself for it.
//!
//! In other words, we only ever configure the current CPU, and, after a timer has fired, CPUs try
//! to steal work from others.
//!
//! In order to make the code more simple, creating a timer doesn't actually do anything. It is
//! only when a timer is polled for the first time that we properly initialize it. This guarantees
//! that all timers in the list have a [`core::task::Waker`] associated to them.
//!

use crate::arch::x86_64::{
    apic::{local, tsc_sync},
    interrupts, pit,
};

use alloc::{collections::VecDeque, sync::Arc};
use core::{
    convert::TryFrom as _,
    num::{NonZeroU32, NonZeroU64},
    pin::Pin,
    sync::atomic,
    task::{Context, Poll, Waker},
    time::Duration,
};
use futures::prelude::*;
use hashbrown::{hash_map::Entry, HashMap};
use spinning_top::Spinlock;

/// Initializes the timers system for x86_64.
pub async fn init(
    local_apics: &'static local::LocalApicsControl,
    pit: &mut pit::PitControl,
) -> Arc<Timers> {
    // TODO: check if TSC is supported somewhere with CPUID.1:EDX.TSC[bit 4] == 1

    // We use the PIT to figure out approximately how many RDTSC ticks happen per second.
    // TODO: instead of using the PIT, we can use CPUID[EAX=0x15] to find the frequency, but that
    // might not be available and does AMD support it?
    let rdtsc_ticks_per_sec = unsafe {
        // We use fences in order to guarantee that the RDTSC instructions don't get moved around.
        // TODO: not sure about these Ordering values
        // TODO: are the fences the same as core::arch::x86_64::_mm_mfence()?
        let before = core::arch::x86_64::_rdtsc();
        atomic::fence(atomic::Ordering::Release);
        pit.timer(Duration::from_secs(1)).await;
        atomic::fence(atomic::Ordering::Acquire);
        let after = core::arch::x86_64::_rdtsc();

        assert!(after > before);
        NonZeroU64::new(after - before).unwrap()
    };

    let monotonic_clock_zero = unsafe { core::arch::x86_64::_rdtsc() };

    Arc::new(Timers {
        local_apics,
        interrupt_vector: interrupts::reserve_any_vector(true).unwrap(),
        monotonic_clock_zero,
        rdtsc_ticks_per_sec,
        next_unique_timer_id: atomic::AtomicU64::new(0),
        monotonic_clock_max: atomic::AtomicU64::new(monotonic_clock_zero),
        shared: Spinlock::new(Shared {
            active_timers: HashMap::with_capacity_and_hasher(16, Default::default()), // TODO: set to number of CPUs
            pending_timers: VecDeque::with_capacity(32), // TODO: which capacity?
        }),
    })
}

/// Timers management for x86/x86_64.
pub struct Timers {
    local_apics: &'static local::LocalApicsControl,

    /// Reservation for an interrupt vector in the interrupts table.
    ///
    /// This is the interrupt that the timer will fire.
    interrupt_vector: interrupts::ReservedInterruptVector,

    /// Number of RDTSC ticks when we initialized the struct. Never modified.
    monotonic_clock_zero: u64,

    /// Approximate number of RDTSC ticks per second. Never modified.
    rdtsc_ticks_per_sec: NonZeroU64,

    /// Each spawned timer has a unique identifier to identify it. This is the identifier of the
    /// next timer to spawn.
    next_unique_timer_id: atomic::AtomicU64,

    /// Since each CPU has its own TSC register, it is possible that they are not always in sync.
    /// If a user calls [`Timers::monotonic_clock`] from one CPU, then calls it again from a
    /// different CPU, we want the value returned the second time to always be superior or equal
    /// to the value returned the first time.
    /// In order to guarantee this, we store here the last returned value of
    /// [`Timers::monotonic_clock`] and make sure to never return a value inferior to this.
    monotonic_clock_max: atomic::AtomicU64,

    /// Everything behind a lock.
    shared: Spinlock<Shared>,
}

/// Everything behind a lock.
struct Shared {
    /// For each CPU, the timer that is currently being configured in its APIC.
    active_timers: HashMap<local::ApicId, TimerEntry, fnv::FnvBuildHasher>,

    /// Timers that aren't being processed by any CPU. Must be picked up.
    ///
    /// Contains the target TSC value and the waker to wake up when it is reached. Always ordered
    /// by ascending TSC value.
    pending_timers: VecDeque<TimerEntry>,
}

/// Timer registered in [`Shared`].
struct TimerEntry {
    /// Identifier of the [`TimerFuture`].
    timer_id: u64,
    /// TSC value to reach before waking up the [`Waker`].
    target_tsc_value: u64,
    /// Waker for when the timer fires.
    waker: Waker,
}

impl Timers {
    /// Returns a `Future` that fires when the given amount of time has elapsed.
    pub fn register_timer(self: &Arc<Self>, duration: Duration) -> TimerFuture {
        // Find out the TSC value corresponding to the requested `Duration`.
        // TODO: don't unwrap
        let tsc_value = duration
            .as_secs()
            .checked_mul(self.rdtsc_ticks_per_sec.get())
            .unwrap()
            .checked_add(
                u64::from(duration.subsec_nanos())
                    .checked_mul(self.rdtsc_ticks_per_sec.get())
                    .unwrap()
                    .checked_div(1_000_000_000)
                    .unwrap(),
            )
            .unwrap();

        TimerFuture {
            timers: self.clone(),
            tsc_value,
            timer_id: None,
        }
    }

    /// Returns the time elapsed since the initialization of this struct.
    ///
    /// Guaranteed to always return a `Duration` greater or equal to the one returned the previous
    /// time.
    pub fn monotonic_clock(&self) -> Duration {
        let rdtsc_value = {
            let local_val = tsc_sync::volatile_rdtsc();
            self.monotonic_clock_max
                .fetch_max(local_val, atomic::Ordering::AcqRel)
                .max(local_val)
        };

        debug_assert!(rdtsc_value >= self.monotonic_clock_zero);
        let diff_ticks = rdtsc_value - self.monotonic_clock_zero;
        let whole_secs = diff_ticks / self.rdtsc_ticks_per_sec.get();
        // TODO: the multiplication below can realistically panic if `rdtsc_ticks_per_sec` is a very large value
        let nanos = 1_000_000_000u64
            .checked_mul(diff_ticks % self.rdtsc_ticks_per_sec.get())
            .unwrap()
            / self.rdtsc_ticks_per_sec.get();
        Duration::new(whole_secs, u32::try_from(nanos).unwrap())
    }
}

/// Future that triggers when the TSC reaches a certain value.
#[must_use]
pub struct TimerFuture {
    /// Reference to the [`Timers`] struct that has created this timer.
    timers: Arc<Timers>,
    /// The TSC value after which the future will be ready.
    tsc_value: u64,
    /// Unique identifier of the timer within the [`Timers`]. `None` if it hasn't been put in the
    /// list yet.
    timer_id: Option<u64>,
}

impl Future for TimerFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let this = &mut *self;

        // Grab the current RDTSC value, after adjustment.
        let now: u64 = {
            let local_val = unsafe { core::arch::x86_64::_rdtsc() };
            this.timers
                .monotonic_clock_max
                .fetch_max(local_val, atomic::Ordering::AcqRel)
                .max(local_val)
        };

        if now >= this.tsc_value {
            return Poll::Ready(());
        }

        // We need either to register the timer in the lists, or update the current registration
        // with the waker passed as parameter.
        let mut shared = this.timers.shared.lock();
        let shared = &mut *shared;

        // Timer is already somewhere in a list.
        if let Some(timer_id) = this.timer_id {
            for active_timer in shared
                .active_timers
                .values_mut()
                .chain(shared.pending_timers.iter_mut())
            {
                if active_timer.timer_id == timer_id {
                    debug_assert_eq!(this.tsc_value, active_timer.target_tsc_value);
                    active_timer.waker = cx.waker().clone();
                    return Poll::Pending;
                }
            }

            // Here is a subtle corner case. It is possible that the target TSC value gets reached
            // while we're waiting to lock `shared` (see above), and that another CPU has detected
            // that and removed the timer from the list as a result.
            // Note that this `assert!` should rather be a `debug_assert!`, but considering the
            // complexity of the whole machinery, we prefer to always detect bugs here.
            assert!(
                this.timers
                    .monotonic_clock_max
                    .load(atomic::Ordering::SeqCst)
                    >= this.tsc_value
            );
            return Poll::Ready(());
        }

        // Timer has never been registered within `shared`.
        // Allocate a new identifier.
        let timer_id = this
            .timers
            .next_unique_timer_id
            .fetch_add(1, atomic::Ordering::Relaxed);
        assert_ne!(timer_id, u64::max_value()); // Check for overflow.
        this.timer_id = Some(timer_id);

        let to_insert = TimerEntry {
            timer_id,
            target_tsc_value: this.tsc_value,
            waker: cx.waker().clone(),
        };

        // Try to insert in `active_timers`.
        let current_apic_id = this.timers.local_apics.current_apic_id();
        if let Entry::Vacant(e) = shared.active_timers.entry(current_apic_id) {
            // Important: we register the waker before configuring the APIC, otherwise the
            // interrupt could fire in-between the two operations.
            this.timers
                .interrupt_vector
                .register_waker(&futures::task::waker(Arc::new(TimerWaker {
                    timers: this.timers.clone(),
                })));
            // TODO: support non-tsc-deadline mode
            this.timers
                .local_apics
                .set_local_timer(local::Timer::TscDeadline {
                    threshold: NonZeroU64::new(this.tsc_value).unwrap(), // TODO: put this is NonZero in the first place
                    vector: this.timers.interrupt_vector.interrupt_num(),
                });
            e.insert(to_insert);
            return Poll::Pending;
        }

        // Otherwise, add as pending.
        shared.pending_timers.push_back(to_insert);
        Poll::Pending
    }
}

/// Waker that is woken up as the outcome of an interrupt.
struct TimerWaker {
    /// [`Timers`] struct this waker belongs to.
    timers: Arc<Timers>,
}

impl futures::task::ArcWake for TimerWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        // Note: week in mind that there in guarantee that this method gets called from a specific
        // CPU. It is possible for a timer interrupt to happen on CPU A, then this function gets
        // called on CPU B.

        // TODO: think about the importance of locking here; is it possibly racy?
        let mut shared = arc_self.timers.shared.lock();

        // Grab the current RDTSC value, after adjustment.
        let now: u64 = {
            let local_val = unsafe { core::arch::x86_64::_rdtsc() };
            arc_self
                .timers
                .monotonic_clock_max
                .fetch_max(local_val, atomic::Ordering::AcqRel)
                .max(local_val)
        };

        // Remove from `active_timers` all the timers that have fired.
        for (_, timer) in shared
            .active_timers
            .drain_filter(|_, timer| timer.target_tsc_value <= now)
        {
            timer.waker.wake();
        }

        let current_apic_id = arc_self.timers.local_apics.current_apic_id();

        // Now process the pending timers.
        loop {
            let next_timer = match shared.pending_timers.pop_front() {
                Some(t) => t,
                None => break,
            };

            if next_timer.target_tsc_value <= now {
                next_timer.waker.wake();
                continue;
            }

            // Try to register the next timer as the current one of the local CPU.
            if let Entry::Vacant(e) = shared.active_timers.entry(current_apic_id) {
                // Important: we register the waker before configuring the APIC, otherwise the
                // interrupt could fire in-between the two operations.
                arc_self
                    .timers
                    .interrupt_vector
                    .register_waker(&futures::task::waker_ref(arc_self));
                // TODO: support non-tsc-deadline mode
                arc_self
                    .timers
                    .local_apics
                    .set_local_timer(local::Timer::TscDeadline {
                        threshold: NonZeroU64::new(next_timer.target_tsc_value).unwrap(), // TODO: put this is NonZero in the first place
                        vector: arc_self.timers.interrupt_vector.interrupt_num(),
                    });
                e.insert(next_timer);
            } else {
                // If the current CPU is already processing a timer, re-add the one we extracted
                // back in the queue.
                shared.pending_timers.push_front(next_timer);
                break;
            }
        }

        // Some memory footprint reduction.
        if shared.pending_timers.is_empty() && shared.pending_timers.capacity() >= 32 {
            shared.pending_timers.shrink_to_fit();
        }
    }
}
