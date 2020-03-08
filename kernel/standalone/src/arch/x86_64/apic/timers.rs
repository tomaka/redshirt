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

use crate::arch::x86_64::{apic::local, interrupts};

use alloc::{collections::VecDeque, sync::Arc};
use core::{
    convert::TryFrom as _,
    num::{NonZeroU32, NonZeroU64},
    ops::Range,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use futures::prelude::*;
use spin::Mutex;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::port::{PortRead as _, PortWrite as _};

pub fn init(local_apics: &local::LocalApicsControl) -> Timers {
    // TODO: check whether CPUID is supported at all?
    // TODO: check whether RDTSC is supported

    let interrupt_vector = interrupts::reserve_any_vector().unwrap();

    // Configure the timer.
    if local_apics.is_tsc_deadline_supported() {
        local_apics.enable_local_timer_interrupt_tsc_deadline(interrupt_vector.interrupt_num());
    } else {
        local_apics.enable_local_timer_interrupt(false, interrupt_vector.interrupt_num());
    }

    Timers {
        local_apics,
        interrupt_vector,
        timers: Mutex::new(VecDeque::with_capacity(32)), // TODO: capacity?
    }
}

pub struct Timers<'a> {
    local_apics: &'a local::LocalApicsControl,

    /// Reservation for an interrupt vector in the interrupts table.
    ///
    /// This is the interrupt that the timer will fire.
    interrupt_vector: interrupts::ReservedInterruptVector,

    /// List of active timers, with the TSC value to reach and the waker to wake. Always ordered
    /// by ascending TSC value.
    ///
    /// The TSC value and the `Waker` stored in the first element of this list must always be
    /// respectively the value that is present in the TSC deadline MSR, and the Waker in the IDT
    /// for the timer's interrupt (with the exception of the interval between when a timer
    /// interrupt has been triggered and when the awakened timer future is being polled).
    // TODO: timers are processor-local, so this is probably wrong
    // TODO: call shrink_to_fit from time to time?
    timers: Mutex<VecDeque<(u64, Waker)>>,
}

impl<'a> Timers<'a> {
    /// Returns a `Future` that fires when the TSC (Timestamp Counter) is superior or equal to
    /// the given value.
    pub fn register_tsc_timer(&self, value: u64) -> TimerFuture {
        TimerFuture {
            timers: self,
            tsc_value: value,
            in_timers_list: false,
        }
    }

    /// Update the state of the APIC with the front of the list.
    fn update_apic_timer_state(
        &self,
        now: u64,
        timers: &mut spin::MutexGuard<VecDeque<(u64, Waker)>>,
    ) {
        if let Some((tsc, waker)) = timers.front() {
            debug_assert!(*tsc > now);
            self.interrupt_vector.register_waker(waker);
            debug_assert_ne!(*tsc, 0); // 0 would disable the timer
            if self.local_apics.is_tsc_deadline_supported() {
                self.local_apics.set_local_tsc_deadline(Some(NonZeroU64::new(*tsc).unwrap()));
            } else {
                let ticks = match u32::try_from(1 + ((*tsc - now) / 128)) {
                    Ok(t) => t,
                    Err(_) => return, // FIXME: properly handle
                };
                self.local_apics.set_local_timer_value(Some(NonZeroU32::new(ticks).unwrap()));
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

const TIMER_MSR: Msr = Msr::new(0x6e0);

// TODO: there's some code duplication for updating the timer value in the APIC
// TODO: is it actually correct to write `desired_tsc - rdtsc` in the one-shot timer register? is the speed matching?

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
                this.timers
                    .update_apic_timer_state(rdtsc, &mut timers);
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
            self.timers
                .update_apic_timer_state(rdtsc, &mut timers);
        }
    }
}
