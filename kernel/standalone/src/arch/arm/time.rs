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

/// Time management on ARM platforms.
///
/// See chapter "B8.1.1 System counter" of the ARM® Architecture Reference Manual
/// (ARMv7-A and ARMv7-R edition).
///
/// The monotonic clock is implemented by reading the `CNTPCT` register.
/// Some characteristics about this register:
///
/// - It is at least 56 bits wide. The value is zero-extended to 64bits.
/// - Roll-over must be no less than 40 years, which is acceptable.
/// - There is no strict requirement on the accuracy, but it is recommended that the timer
///   does not gain or lose more than 10 seconds in a 24 hours period.
///
// TODO: it is unclear whether the counter is global, or per CPU. The manual mentions,
//       however, it is impossible to observe time rolling back even across CPUs
use alloc::{sync::Arc, vec::Vec};
use core::{
    convert::TryFrom as _,
    num::NonZeroUsize,
    pin::Pin,
    sync::atomic,
    task::{Context, Poll, Waker},
};
use futures::prelude::*;
use spin::Mutex;

/// All the time-related functionalities.
pub struct TimeControl {
    /// List of timers, ordered by ascending deadline.
    ///
    /// This is a subset of the list of alive [`TimerFuture`]s created through this
    /// [`TimeControl`]. All the elements of this list correspond to a [`TimerFuture`], but not
    /// all [`TimerFuture`]s have an entry in this list.
    ///
    /// The first timer in the list must always match what the hardware timer is configured for.
    /// Any operation that modifies the first element must also update the hardware timer by
    /// calling [`update_hardware`].
    timers: Mutex<Vec<Timer>>,

    /// Each timer in [`TimerControl::timers`] is assigned a private id for identification. This
    /// counter increases linearly and contains the ID to assign to the next timer to put in the
    /// list.
    next_timer_id: atomic::AtomicUsize,
}

struct Timer {
    id: NonZeroUsize,
    /// Value of the physical counter that the timer wants to reach.
    counter_value: u64,
    /// Waker to wake when the timer is fired.
    waker: Option<Waker>,
}

/// Implementation of the `Future` trait returned by [`TimeControl::timer`].
pub struct TimerFuture {
    /// Time controller this future has been created from.
    time_control: Arc<TimeControl>,
    /// Identifier assigned to this timer, or `None` if this timer is not in
    /// [`TimerControl::timers`].
    id: Option<NonZeroUsize>,
    /// Value of the physical counter that the timer wants to reach.
    counter_value: u64,
}

/// Value that we put in the CNTFRQ register on start-up.
///
/// Frequency in Hz. The value of the ARM hardware counter is increased by this value every
/// second.
// TODO: if the timer is only 56bits, then this will overflow after 388 days, which is a bit short
//       for prime time
const CNTFRQ: u32 = 0x80000000;

impl TimeControl {
    /// Initializes the time control system.
    ///
    /// # Safety
    ///
    /// No other code must access the ARM system-timer-related registers for as long as the
    /// `TimeControl` is alive.
    ///
    pub unsafe fn init() -> Arc<TimeControl> {
        // Initialize the physical counter frequency.
        // TODO: I think this is a global setting, but make sure it's the case?
        asm!("mcr p15, 0, $0, c14, c0, 0"::"r"(CNTFRQ)::"volatile");

        // TODO: this code doesn't work, as we have to register some IRQ handler or something to
        //       check the state of the timers and fire the wakers

        Arc::new(TimeControl {
            timers: Mutex::new(Vec::new()),
            next_timer_id: From::from(1),
        })
    }

    /// Implementation suitable for [`arch::PlatformSpecific::monotonic_clock`].
    pub fn monotonic_clock(self: &Arc<Self>) -> u128 {
        let counter_value = physical_counter();

        // We have to turn this into a number of nanoseconds.
        1_000_000_000 * u128::from(counter_value) / u128::from(CNTFRQ)
    }

    /// Implementation suitable for [`arch::PlatformSpecific::timer`].
    pub fn timer(self: &Arc<Self>, deadline: u128) -> TimerFuture {
        // Since `deadline` is a number of nanoseconds, we have to find the value of the physical
        // counter that corresponds to it.
        //
        // Note that in case of an overflow, we cap to the maximum value. The maximum value is
        // expected to never be reached. TODO: this isn't necessarily true
        let counter_value = u64::try_from(deadline * u128::from(CNTFRQ) / 1_000_000_000)
            .unwrap_or(u64::max_value());

        // Note that we don't immediately put the timer in our list of timers. This is done
        // lazily because:
        //
        // - If the deadline has already passed, we immediately return `Ready` without going
        //   through the process of adding an entry and removing it.
        // - If the deadline has no already passed, polling will need to update the list anyway
        //   in order to update the `Waker`. We might therefore as well just insert the value
        //   then.

        TimerFuture {
            time_control: self.clone(),
            id: None,
            counter_value,
        }
    }
}

impl Future for TimerFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let this = &mut *self;

        let cnt_value = physical_counter();
        if cnt_value >= this.counter_value {
            return Poll::Ready(());
        }

        // TODO: the timer should asynchronously wait for a lock, rather than spin-lock the mutex
        //       this is too difficult to do before https://github.com/rust-lang/rust/issues/56974

        // If we have already registered ourselves, update the waker.
        if let Some(id) = this.id {
            let mut timers = this.time_control.timers.lock();
            let mut my_entry = timers.iter_mut().find(|t| t.id == id).unwrap();
            my_entry.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        // If the timer hasn't registered itself in the list of timers of the `time_control`,
        // do so now.
        let assigned_id = {
            let assigned_id = this
                .time_control
                .next_timer_id
                .fetch_add(1, atomic::Ordering::Relaxed);
            assert_ne!(assigned_id, usize::max_value());
            assert_ne!(assigned_id, 0);
            NonZeroUsize::new(assigned_id).unwrap()
        };

        let to_insert = Timer {
            id: assigned_id,
            counter_value: this.counter_value,
            waker: Some(cx.waker().clone()),
        };

        let mut timers = this.time_control.timers.lock();
        if let Some(pos) = timers
            .iter()
            .position(|t| t.counter_value > this.counter_value)
        {
            timers.insert(pos, to_insert);
            if pos == 0 {
                update_hardware(&mut timers);
            }
        } else {
            let was_empty = timers.is_empty();
            timers.push(to_insert);
            if was_empty {
                update_hardware(&mut timers);
            }
        }

        this.id = Some(assigned_id);

        Poll::Pending
    }
}

impl Drop for TimerFuture {
    fn drop(&mut self) {
        if let Some(my_id) = self.id {
            // Clean up behind us.
            let mut timers = self.time_control.timers.lock();

            // It is possible that we are not in the list anymore, in case a different timer has.  TODO: is that true? finish
            if let Some(pos) = timers.iter().position(|t| t.id == my_id) {
                timers.remove(pos);
                if pos == 0 {
                    update_hardware(&mut timers);
                }
            }
        }
    }
}

/// Reads the value of the `CNTPCT` register.
fn physical_counter() -> u64 {
    unsafe {
        let lo: u32;
        let hi: u32;
        asm!("mrrc p15, 0, $0, $1, c14": "=r"(lo), "=r"(hi) ::: "volatile");
        u64::from(hi) << 32 | u64::from(lo)
    }
}

/// Updates the state of the hardware timer according to the content of `timers`.
fn update_hardware(timers: &mut Vec<Timer>) {
    unsafe {
        // See chapter "B8.1.5 Timers" of the ARM® Architecture Reference Manual (ARMv7-A and
        // ARMv7-R edition).

        // If there's no active timer, disable the timer firing by updating the `CNTP_CTL`
        // register.
        if timers.is_empty() {
            asm!("mcr p15, 0, $0, c14, c2, 1" :: "r"(0));
            return;
        }

        // Make sure that the timer is enabled by updating the `CNTP_CTL` register.
        // TODO: don't do this every single time
        asm!("mcr p15, 0, $0, c14, c2, 1" :: "r"(0b01));

        // Write the `CNTP_CVAL` register with the value to compare with.
        // The timer will fire when the physical counter (`CNTPCT`) reaches the given value.
        {
            let cmp_value = timers.get(0).unwrap().counter_value;
            let lo = u32::try_from(cmp_value & 0xffffffff).unwrap();
            let hi = u32::try_from(cmp_value >> 32).unwrap();
            asm!("mcrr p15, 2, $0, $1, c14" :: "r"(lo), "r"(hi));
        }
    }
}
