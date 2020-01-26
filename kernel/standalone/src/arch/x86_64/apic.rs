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
pub unsafe fn init() -> ApicControl {
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

    ApicControl {
        apic_base_addr,
        timers: Arc::new(Mutex::new(VecDeque::with_capacity(32))), // TODO: capacity?
        tsc_deadline,
    }
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
    timers: Arc<Mutex<VecDeque<(u64, Waker)>>>,

    /// If true, then we use TSC deadline mode. If false, we use one-shot timers.
    tsc_deadline: bool,
}

impl ApicControl {
    /// Returns a `Future` that fires when the TSC (Timestamp Counter) is superior or equal to
    /// the given value.
    pub fn register_tsc_timer(&self, value: u64) -> impl Future<Output = ()> {
        TscTimerFuture {
            tsc_value: value,
            timers: self.timers.clone(),
            in_timers_list: false,
            apic_base_addr: self.apic_base_addr,
            tsc_deadline: self.tsc_deadline,
        }
    }
}

/// Future that triggers when the MSR reaches a certain value.
#[must_use]
pub struct TscTimerFuture {
    /// The TSC value after which the future will be ready.
    tsc_value: u64,
    /// List of timers. Clone of what is in `ApicControl`. Shared between all timers
    timers: Arc<Mutex<VecDeque<(u64, Waker)>>>,
    /// If true, then we are in the list of timers of the `ApicControl`.
    in_timers_list: bool,
    /// Base address of the APIC in memory.
    apic_base_addr: u64,
    /// If true, then we use TSC deadline mode. If false, we use one-shot timers.
    tsc_deadline: bool,
}

const TIMER_MSR: Msr = Msr::new(0x6e0);

// TODO: there's some code duplication for updating the timer value in the APIC
// TODO: is it actually correct to write `desired_tsc - rdtsc` in the one-shot timer register? is the speed matching?

impl Future for TscTimerFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let mut this = &mut *self;

        let rdtsc = unsafe { core::arch::x86_64::_rdtsc() };
        if rdtsc >= this.tsc_value && !this.in_timers_list {
            return Poll::Ready(());
        }

        let mut timers = this.timers.lock();

        if rdtsc >= this.tsc_value {
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
                if let Some((tsc, waker)) = timers.front() {
                    debug_assert!(*tsc > rdtsc);
                    interrupts::set_interrupt_waker(50, waker); // TODO: 50?
                    debug_assert_ne!(*tsc, 0); // 0 would disable the timer
                    if this.tsc_deadline {
                        unsafe {
                            TIMER_MSR.write(*tsc);
                        }
                    } else {
                        unsafe {
                            let init_cnt_addr =
                                usize::try_from(this.apic_base_addr + 0x380).unwrap() as *mut u32;
                            init_cnt_addr.write_volatile(u32::try_from((*tsc - rdtsc) / 128).unwrap());
                        }
                    }
                }
            }

            return Poll::Ready(());
        }

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
            interrupts::set_interrupt_waker(50, cx.waker()); // TODO: 50?
            debug_assert_ne!(this.tsc_value, 0); // 0 would disable the timer
            if this.tsc_deadline {
                unsafe {
                    TIMER_MSR.write(this.tsc_value);
                }
            } else {
                unsafe {
                    let init_cnt_addr =
                        usize::try_from(this.apic_base_addr + 0x380).unwrap() as *mut u32;
                    init_cnt_addr.write_volatile(u32::try_from((this.tsc_value - rdtsc) / 128).unwrap());
                }
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
        let mut timers = self.timers.lock();
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
            if let Some((tsc, waker)) = timers.front() {
                interrupts::set_interrupt_waker(50, waker); // TODO: 50?
                debug_assert_ne!(*tsc, 0); // 0 would disable the timer
                if self.tsc_deadline {
                    unsafe {
                        TIMER_MSR.write(*tsc);
                    }
                } else {
                    unsafe {
                        let rdtsc = core::arch::x86_64::_rdtsc();
                        let init_cnt_addr =
                            usize::try_from(self.apic_base_addr + 0x380).unwrap() as *mut u32;
                        init_cnt_addr.write_volatile(u32::try_from((*tsc - rdtsc) / 128).unwrap());
                    }
                }
            }
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
