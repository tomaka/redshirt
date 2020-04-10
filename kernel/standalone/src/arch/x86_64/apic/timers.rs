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
//! certain amount of timer has elapsed. The PIT is unique on the system.
//! - x86/x86_64 CPUs optionally provide a register named TSC (TimeStamp Counter) whose value is
//! accessible using the `RDTSC` instruction. The value is increased at a uniform rate, even if
//! the processor is halted (`hlt` instruction). Each CPU on the system has a separate TSC value.
//! - Each CPU also has a local APIC that can trigger an interrupt after a certain number of
//! timer cycles has passed, or, if supported, when the TSC reaches a certain value. Each CPU has
//! its own local APIC, and the interrupt will only concern this CPU in particular.
//! - The HPET is a more recent version of the PIT.
//!
//! # Timers management
//!
//! ## One timer at a time
//!
//! In order to fire a single timer after a certain duration, we use the TSC as a reference point.
//! As part of the initialization process, we measure the rate at which the TSC increases, and
//! thus can determine at which TSC value the requested duration will have elapsed.
//!
//! We then use the local APIC in TSC deadline value if supported, or regular mode if not.
//!
//! In regular mode, considering that the timer value is 32bits, it might be necessary to chain
//! multiple timers before the desired TSC value is reached.
//!
//! By using the TSC as the reference value, we can check at any time whether the timer has been
//! fired. The local APIC's timer is use solely for its capability of waking up a halted CPU.
//!
//! Keep in mind, though, that each CPU has a different TSC value.
// TODO: yeah that's actually a problem ^
//!
//! ## Multiple timers
//!
//! In a perfect world we would like to either distribute timers uniformly amongst the multiple
//! CPUs, so that the overhead of handling interrupts is distributed uniformly, or alternatively
//! we would like to setup a timer on the CPU that is actually waiting for the timer to be
//! resolved.
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
//!   - If the current CPU is not currently handling any timer, add the newly-created timer to
//!     the shared list.
//! - When a timer interrupt is fired, the CPU that has been interrupted picks the next pending
//!   timer from the shared list and configures itself for it.
//! - To cancel a timer:
//!    - If the timer is in the list, remove it.
//!    - If the timer is being handled by a CPU, don't do anything.
//!

use crate::arch::x86_64::{apic::local, interrupts, pit};

use alloc::collections::VecDeque;
use core::{
    convert::TryFrom as _,
    num::{NonZeroU32, NonZeroU64},
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};
use futures::prelude::*;
use spinning_top::Spinlock;

/// Initializes the timers system for x86_64.
pub async fn init<'a>(
    local_apics: &'a local::LocalApicsControl,
    pit: &mut pit::PitControl,
) -> Timers<'a> {
    // TODO: check if TSC is supported somewhere with CPUID.1:EDX.TSC[bit 4] == 1

    // We use the PIT to figure out approximately how many RDTSC ticks happen per second.
    // TODO: instead of using the PIT, we can use CPUID[EAX=0x15] to find the frequency, but that
    // might not be available and does AMD support it?
    let rdtsc_ticks_per_sec = unsafe {
        // We use fences in order to guarantee that the RDTSC instructions don't get moved around.
        // TODO: not sure about these Ordering values
        // TODO: are the fences the same as core::arch::x86_64::_mm_mfence()?
        let before = core::arch::x86_64::_rdtsc();
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        pit.timer(Duration::from_secs(1)).await;
        core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
        let after = core::arch::x86_64::_rdtsc();

        assert!(after > before);
        after - before
    };

    Timers {
        local_apics,
        interrupt_vector: interrupts::reserve_any_vector(true).unwrap(),
        monotonic_clock_zero: unsafe { core::arch::x86_64::_rdtsc() },
        rdtsc_ticks_per_sec,
        timers: Spinlock::new(VecDeque::with_capacity(32)), // TODO: capacity?
    }
}

pub struct Timers<'a> {
    local_apics: &'a local::LocalApicsControl,

    /// Reservation for an interrupt vector in the interrupts table.
    ///
    /// This is the interrupt that the timer will fire.
    interrupt_vector: interrupts::ReservedInterruptVector,

    /// Number of RDTSC ticks when we initialized the struct.
    monotonic_clock_zero: u64,

    /// Approximate number of RDTSC ticks per second.
    rdtsc_ticks_per_sec: u64,

    /// List of active timers, with the TSC value to reach and the waker to wake. Always ordered
    /// by ascending TSC value.
    ///
    /// The TSC value and the `Waker` stored in the first element of this list must always be
    /// respectively the value that is present in the TSC deadline MSR, and the Waker in the IDT
    /// for the timer's interrupt (with the exception of the interval between when a timer
    /// interrupt has been triggered and when the awakened timer future is being polled).
    // TODO: timers are processor-local, so this is probably wrong
    // TODO: call shrink_to_fit from time to time?
    timers: Spinlock<VecDeque<(u64, Waker)>>,
}

impl<'a> Timers<'a> {
    /// Returns a `Future` that fires when the given amount of time has elapsed.
    pub fn register_timer(&self, duration: Duration) -> TimerFuture {
        // TODO: don't unwrap
        let tsc_value = duration
            .as_secs()
            .checked_mul(self.rdtsc_ticks_per_sec)
            .unwrap()
            .checked_add(
                u64::from(duration.subsec_nanos())
                    .checked_mul(self.rdtsc_ticks_per_sec)
                    .unwrap()
                    .checked_div(1_000_000_000)
                    .unwrap(),
            )
            .unwrap();

        TimerFuture {
            timers: self,
            tsc_value,
            in_timers_list: false,
        }
    }

    pub fn monotonic_clock(&self) -> Duration {
        let now = unsafe { core::arch::x86_64::_rdtsc() };
        // TODO: is it correct to have monotonic_clock_zero determined from the main thread,
        // then compared with the RDTSC of other CPUs?
        // TODO: check all the math operations here
        debug_assert!(now >= self.monotonic_clock_zero);
        let diff_ticks = now - self.monotonic_clock_zero;
        let whole_secs = diff_ticks / self.rdtsc_ticks_per_sec;
        let nanos =
            1_000_000_000 * (diff_ticks % self.rdtsc_ticks_per_sec) / self.rdtsc_ticks_per_sec;
        Duration::new(whole_secs, u32::try_from(nanos).unwrap())
    }

    /// Update the state of the APIC with the front of the list.
    fn update_apic_timer_state(
        &self,
        now: u64,
        timers: &mut spinning_top::SpinlockGuard<VecDeque<(u64, Waker)>>,
    ) {
        if let Some((tsc, waker)) = timers.front() {
            debug_assert!(*tsc > now);
            self.interrupt_vector.register_waker(waker);
            debug_assert_ne!(*tsc, 0); // 0 would disable the timer
            if self.local_apics.is_tsc_deadline_supported() {
                self.local_apics
                    .set_local_tsc_deadline(Some(NonZeroU64::new(*tsc).unwrap()));
            } else {
                let ticks = match u32::try_from(1 + ((*tsc - now) / 128)) {
                    Ok(t) => t,
                    Err(_) => return, // FIXME: properly handle
                };
                self.local_apics
                    .set_local_timer_value(Some(NonZeroU32::new(ticks).unwrap()));
            }
        }
    }
}

/// Future that triggers when the TSC reaches a certain value.
//
// # Implementation information
//
// The way this `Future` works is that it inserts itself in the list of timers when first polled.
// The head of the list of timers must always be in sync with the state of the APIC, and as such
// we update the state of the APIC if we modify what the first element is.
//
// When a timer interrupt fires, we need to update the state of the APIC for the next timer. To
// do so, the implementation assumes that the `TimerFuture` corresponding to timer that has
// fired will either be polled or destroyed.
//
#[must_use]
pub struct TimerFuture<'a> {
    /// Reference to the [`Timers`] struct.
    timers: &'a Timers<'a>,
    /// The TSC value after which the future will be ready.
    tsc_value: u64,
    /// If true, then we are in the list of timers of the `ApicControl`.
    in_timers_list: bool,
}

// TODO: there's some code duplication for updating the timer value in the APIC

impl<'a> Future for TimerFuture<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let this = &mut *self;

        let rdtsc = unsafe { core::arch::x86_64::_rdtsc() };
        if rdtsc >= this.tsc_value {
            if !this.in_timers_list {
                return Poll::Ready(());
            }

            let mut timers = this.timers.timers.lock();

            // If we were in the list, then we need to remove ourselves from it. We also remove
            // all the earlier timers. It is consequently also possible that a different timer has
            // already removed ourselves.
            let mut removed_any = false;
            while timers
                .front()
                .map(|(tsc, _)| *tsc <= rdtsc)
                .unwrap_or(false)
            {
                let (_, waker) = timers.pop_front().unwrap();
                removed_any = true;
                if !waker.will_wake(cx.waker()) {
                    waker.wake();
                }
            }

            // It is important that we update this, for the Drop implementation.
            this.in_timers_list = false;

            // If we updated the head of the timers list, we need to update the MSR and waker.
            if removed_any {
                this.timers.update_apic_timer_state(rdtsc, &mut timers);
            }

            return Poll::Ready(());
        }

        // We haven't reached the target timestamp yet.
        debug_assert!(rdtsc < this.tsc_value);

        if !this.in_timers_list {
            let mut timers = this.timers.timers.lock();

            // Position where to insert the new timer in the list.
            // We use `>` rather than `>=` so that `insert_position` is not 0 if the first element
            // has the same value.
            let insert_position = timers
                .iter()
                .position(|(v, _)| *v > this.tsc_value)
                .unwrap_or(0);

            timers.insert(insert_position, (this.tsc_value, cx.waker().clone()));
            this.in_timers_list = true;

            // If we update the head of the timers list, we need to update the MSR and waker.
            if insert_position == 0 {
                this.timers.update_apic_timer_state(rdtsc, &mut timers);
            }
        }

        Poll::Pending
    }
}

impl<'a> Drop for TimerFuture<'a> {
    fn drop(&mut self) {
        if !self.in_timers_list {
            return;
        }

        // We need to unregister ourselves. It is possible that a different timer has already
        // removed us from the list.
        let mut timers = self.timers.timers.lock();
        let my_position = match timers.iter().position(|(v, _)| *v == self.tsc_value) {
            Some(p) => p,
            None => return,
        };

        // In the unlikely event that there are multiple timers with the same value in a row,
        // we prefer to not do anything and let other timers do the clean up later.
        if timers
            .get(my_position + 1)
            .map(|(v, _)| *v == self.tsc_value)
            .unwrap_or(false)
        {
            return;
        }

        timers.remove(my_position);

        // If we update the head of the timers list, we need to update the MSR and waker.
        if my_position == 0 {
            let rdtsc = unsafe { core::arch::x86_64::_rdtsc() };
            self.timers.update_apic_timer_state(rdtsc, &mut timers);
        }
    }
}
