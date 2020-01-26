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

use crate::arch::x86_64::interrupts;

use alloc::{collections::VecDeque, sync::Arc};
use core::{
    convert::TryFrom as _,
    ops::Range,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use futures::prelude::*;
use spin::Mutex;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::port::{PortRead as _, PortWrite as _};

// TODO: init() has to be called; this isn't great

///
///
/// # Safety
///
/// Must only be called once, and assumes that no other piece of code reads or writes to the
/// registers related to the APIC.
///
/// > **Note**: The term "registers related to the APIC" is very loosely defined, but no ambiguity
/// >           has been encountered so far.
///
pub unsafe fn init() -> Arc<ApicControl> {
    init_pic();

    // TODO: check whether CPUID is supported at all?

    // TODO: handle properly?
    // TODO: check whether RDTSC is supported
    assert!(is_apic_supported());

    // TODO: carefully read volume 3 chapter 10.4 of the Intel manual and see if we're doing things correctly

    // TODO: this is all processor-local and needs to be done once per processor

    // Set up the APIC.
    let apic_base_addr = {
        const APIC_BASE_MSR: Msr = Msr::new(0x1b);
        let base_addr = APIC_BASE_MSR.read() & !0xfff;
        APIC_BASE_MSR.write(base_addr | 0x800); // Enable the APIC.
        base_addr
    };

    // Enable spurious interrupts.
    {
        let svr_addr = usize::try_from(apic_base_addr + 0xf0).unwrap() as *mut u32;
        let val = svr_addr.read_volatile();
        svr_addr.write_volatile(val | 0x100); // Enable spurious interrupts.
    }

    // TODO: configure the error handling interrupt?

    let tsc_deadline = is_tsc_deadline_supported();

    // Configure the timer.
    {
        let timer_lvt_addr = usize::try_from(apic_base_addr + 0x320).unwrap() as *mut u32;
        if tsc_deadline {
            timer_lvt_addr.write_volatile((0b10 << 17) | 50); // TSC deadline and interrupt vector 50 // TODO: why 50?
        } else {
            timer_lvt_addr.write_volatile(50); // One-shot and interrupt vector 50 // TODO: why 50?

            let divide_config_addr = usize::try_from(apic_base_addr + 0x3e0).unwrap() as *mut u32;
            divide_config_addr.write_volatile(0b1010); // Divide by 128
        }
    }

    Arc::new(ApicControl {
        apic_base_addr,
        timers: Mutex::new(VecDeque::with_capacity(32)), // TODO: capacity?
        tsc_deadline,
    })
}

/// Stores state used to control the APIC.
pub struct ApicControl {
    /// Base address of the APIC in memory.
    apic_base_addr: u64,

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

    /// If true, then we use TSC deadline mode. If false, we use one-shot timers.
    tsc_deadline: bool,
}

/// Opaque type representing the APIC ID of a processor.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ApicId(u8);

impl From<u8> for ApicId {
    fn from(val: u8) -> ApicId {
        ApicId(val)
    }
}

impl ApicControl {
    /// Returns a `Future` that fires when the TSC (Timestamp Counter) is superior or equal to
    /// the given value.
    pub fn register_tsc_timer(self: &Arc<Self>, value: u64) -> TscTimerFuture {
        TscTimerFuture {
            apic_control: self.clone(),
            tsc_value: value,
            in_timers_list: false,
        }
    }

    /// Returns the [`ApicId`] of the calling processor.
    pub fn current_apic_id(self: &Arc<Self>) -> ApicId {
        unsafe {
            // Note: this is correct because we never modify the local APIC ID.
            let apic_id_addr = usize::try_from(self.apic_base_addr + 0x20).unwrap() as *mut u32;
            let apic_id = u8::try_from(apic_id_addr.read_volatile() >> 24).unwrap();
            ApicId(apic_id)
        }
    }

    /// Causes the processor with the target APIC ID to wake up.
    ///
    /// # Panic
    ///
    /// Panics if the interrupt vector is inferior to 32.
    /// Panics if the `target_apic_id` is out of range. // TODO: <--
    /// Panics if the processed with the `target_apic_id` hasn't started yet. // TODO: <--
    ///
    pub fn send_interprocessor_interrupt(self: &Arc<Self>, target_apic_id: ApicId, vector: u8) {
        // TODO: if P6 architecture, then only 4 bits of the target are valid; do we care about that?
        let value_lo = u32::from(vector);
        let value_hi = u32::from(target_apic_id.0) << (56 - 32);

        let value_lo_addr = usize::try_from(self.apic_base_addr + 0x300).unwrap() as *mut u32;
        let value_hi_addr = usize::try_from(self.apic_base_addr + 0x310).unwrap() as *mut u32;

        // TODO: assert!(target_api_id < ...);
        assert!(vector >= 32);

        // We want the write to be atomic.
        unsafe {
            if x86_64::instructions::interrupts::are_enabled() {
                x86_64::instructions::interrupts::disable();
                value_hi_addr.write_volatile(value_hi);
                value_lo_addr.write_volatile(value_lo);
                x86_64::instructions::interrupts::enable();
            } else {
                value_hi_addr.write_volatile(value_hi);
                value_lo_addr.write_volatile(value_lo);
            }
        }
    }

    /// Update the state of the APIC with the front of the list.
    fn update_apic_timer_state(
        self: &Arc<Self>,
        now: u64,
        timers: &mut spin::MutexGuard<VecDeque<(u64, Waker)>>,
    ) {
        if let Some((tsc, waker)) = timers.front() {
            debug_assert!(*tsc > now);
            interrupts::set_interrupt_waker(50, waker); // TODO: 50?
            debug_assert_ne!(*tsc, 0); // 0 would disable the timer
            if self.tsc_deadline {
                unsafe {
                    TIMER_MSR.write(*tsc);
                }
            } else {
                unsafe {
                    let init_cnt_addr =
                        usize::try_from(self.apic_base_addr + 0x380).unwrap() as *mut u32;
                    let ticks = match u32::try_from(1 + ((*tsc - now) / 128)) {
                        Ok(t) => t,
                        Err(_) => return, // FIXME: properly handle
                    };
                    init_cnt_addr.write_volatile(ticks);
                }
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
// do so, the implementation assumes that the `TscTimerFuture` corresponding to timer that has
// fired will either be polled or destroyed.
//
#[must_use]
pub struct TscTimerFuture {
    /// Reference to the APIC controller.
    apic_control: Arc<ApicControl>,
    /// The TSC value after which the future will be ready.
    tsc_value: u64,
    /// If true, then we are in the list of timers of the `ApicControl`.
    in_timers_list: bool,
}

const TIMER_MSR: Msr = Msr::new(0x6e0);

// TODO: there's some code duplication for updating the timer value in the APIC
// TODO: is it actually correct to write `desired_tsc - rdtsc` in the one-shot timer register? is the speed matching?

impl Future for TscTimerFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let this = &mut *self;

        let rdtsc = unsafe { core::arch::x86_64::_rdtsc() };
        if rdtsc >= this.tsc_value {
            if !this.in_timers_list {
                return Poll::Ready(());
            }

            let mut timers = this.apic_control.timers.lock();

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
                this.apic_control
                    .update_apic_timer_state(rdtsc, &mut timers);
            }

            return Poll::Ready(());
        }

        // We haven't reached the target timestamp yet.
        debug_assert!(rdtsc < this.tsc_value);

        if !this.in_timers_list {
            let mut timers = this.apic_control.timers.lock();

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
                this.apic_control
                    .update_apic_timer_state(rdtsc, &mut timers);
            }
        }

        Poll::Pending
    }
}

impl Drop for TscTimerFuture {
    fn drop(&mut self) {
        if !self.in_timers_list {
            return;
        }

        // We need to unregister ourselves. It is possible that a different timer has already
        // removed us from the list.
        let mut timers = self.apic_control.timers.lock();
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
            self.apic_control
                .update_apic_timer_state(rdtsc, &mut timers);
        }
    }
}

/// Checks in the CPUID whether the APIC is supported.
fn is_apic_supported() -> bool {
    unsafe {
        let cpuid = core::arch::x86_64::__cpuid(0x1);
        cpuid.edx & (1 << 9) != 0
    }
}

/// Checks in the CPUID whether the APIC timer supports TSC deadline mode.
fn is_tsc_deadline_supported() -> bool {
    unsafe {
        let cpuid = core::arch::x86_64::__cpuid(0x1);
        cpuid.ecx & (1 << 24) != 0
    }
}

/// Remap and disable the PIC.
///
/// The PIC (Programmable Interrupt Controller) is the old chip responsible for triggering
/// on the CPU interrupts coming from the hardware.
///
/// Because of poor design decisions, it will by default trigger interrupts 0 to 15 on the CPU,
/// which are normally reserved for software-related concerns. For example, the timer will by
/// default trigger interrupt 8, which is also the double fault exception handler.
///
/// In order to solve this issue, one has to reconfigure the PIC in order to make it trigger
/// interrupts between 32 and 47 rather than 0 to 15.
///
/// Note that this code disables the PIC altogether. Despite the PIC being disabled, it is
/// still possible to receive spurious interrupts. Hence the remapping.
unsafe fn init_pic() {
    u8::write_to_port(0xa1, 0xff);
    u8::write_to_port(0x21, 0xff);
    u8::write_to_port(0x20, 0x10 | 0x01);
    u8::write_to_port(0xa0, 0x10 | 0x01);
    u8::write_to_port(0x21, 0x20);
    u8::write_to_port(0xa1, 0x28);
    u8::write_to_port(0x21, 4);
    u8::write_to_port(0xa1, 2);
    u8::write_to_port(0x21, 0x01);
    u8::write_to_port(0xa1, 0x01);
    u8::write_to_port(0xa1, 0xff);
    u8::write_to_port(0x21, 0xff);
}
