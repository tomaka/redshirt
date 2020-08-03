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

use super::super::interrupts;

use alloc::sync::Arc;
use core::{
    convert::TryFrom as _,
    num::{NonZeroU32, NonZeroU64},
};
use x86_64::registers::model_specific::Msr;

// TODO: "For correct APIC operation, this address space must be mapped to an area of memory that has been designated as strong uncacheable (UC)"
//       For now everything is Strong Uncachable anyway, but care must be taken once we properly
//       handle caching.

/// Represents the local APICs of all CPUs.
pub struct LocalApicsControl {
    /// True if the CPUs support TSC-Deadline mode.
    tsc_deadline_supported: bool,
    /// Interrupt vector triggered when an APIC error happens.
    error_interrupt_vector: interrupts::ReservedInterruptVector,
}

/// Opaque type representing the APIC ID of a processor.
///
/// Since we never modify the APIC ID of processors, an instance of this struct is a guarantee
/// that a processor with the given ID exists.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ApicId(u8);

/// Makes sure that the current CPU supports an APIC.
///
/// After this, you have to call [`LocalApicsControl::init_local`].
///
/// # Panic
///
/// Panics if the CPU doesn't support the APIC.
///
/// # Safety
///
/// Must only be initialized once, and assumes that no other piece of code reads or writes
/// to the MSR registers related to the local APIC or x2APIC, or the registers mapped to
/// physical memory.
///
pub unsafe fn init() -> LocalApicsControl {
    // We don't support platforms without an APIC or without the TSC.
    assert!(is_apic_supported());
    assert!(is_tsc_supported());

    // We reserve an interrupt vector for errors triggered by the APIC.
    // Each APIC, when it gets initialized, will set this interrupt vector in its LVT.
    let error_interrupt_vector = interrupts::reserve_any_vector(240).unwrap();
    // We don't have any intent of actually processing the interrupts, instead we just set up a
    // one-time panicking waker.
    error_interrupt_vector.register_waker(&{
        struct ErrWaker;
        impl futures::task::ArcWake for ErrWaker {
            fn wake_by_ref(_: &Arc<Self>) {
                unsafe {
                    // The errors reported are found in the Error Status Register (ESR).
                    let esr_addr = usize::try_from(APIC_BASE_ADDR + 0xf0).unwrap() as *mut u32;
                    // Before reading from the ESR, we must first write to it.
                    esr_addr.write_volatile(0b11111111);
                    let status = esr_addr.read_volatile();
                    panic!(
                        "Local APIC error; Error Status Register value: 0x{:x}",
                        status
                    );
                }
            }
        }
        futures::task::waker(Arc::new(ErrWaker))
    });

    LocalApicsControl {
        tsc_deadline_supported: is_tsc_deadline_supported(),
        error_interrupt_vector,
    }
}

// TODO: bad API ; should be a method on LocalApisControl, and a &'static ref passed when
// initializing the IDT
// TODO: document that no mutex is being locked; important because it's called from within an
// interrupt handler
pub unsafe fn end_of_interrupt() {
    let addr = usize::try_from(APIC_BASE_ADDR + 0xB0).unwrap() as *mut u32;
    addr.write_volatile(0x0);
}

pub enum Timer {
    /// Default state.
    Disabled,
    Timer {
        /// Number of ticks before the timer triggers.
        value: NonZeroU32,
        /// `value` will be multiplied by `value_multiplier`.
        /// Must be a power of two.
        value_multiplier: u8,
        /// If `true`, the timer will continue firing periodically.
        /// If `false`, it will switch back to `Disabled` after the first trigger.
        periodic: bool,
        /// Interrupt vector to trigger when the timer fires.
        vector: u8,
    },
    TscDeadline {
        /// Timer fires when the `rdtsc` value goes over this threshold.
        threshold: NonZeroU64,
        /// Interrupt vector to trigger when the timer fires.
        vector: u8,
    },
}

impl ApicId {
    /// Builds an [`ApicId`] from a raw identifier without checking the value.
    ///
    /// # Safety
    ///
    /// There must be a processor with the given APIC ID.
    ///
    pub const unsafe fn from_unchecked(val: u8) -> Self {
        ApicId(val)
    }

    /// Returns the integer value of this ID.
    pub const fn get(&self) -> u8 {
        self.0
    }
}

impl LocalApicsControl {
    /// Initializes the APIC of the local CPU.
    ///
    /// # Safety
    ///
    /// Must only be called once per CPU.
    ///
    // TODO: add debug_assert!s in all the other methods that check if the local APIC is initialized
    pub unsafe fn init_local(&self) {
        assert!(is_apic_supported());
        assert_eq!(self.tsc_deadline_supported, is_tsc_deadline_supported());

        // Set up the APIC.
        {
            const APIC_BASE_MSR: Msr = Msr::new(0x1b);
            let base_addr = APIC_BASE_MSR.read() & !0xfff;
            // We never re-map the APIC. For safety, we ensure that nothing weird has happened here.
            assert_eq!(usize::try_from(base_addr), Ok(APIC_BASE_ADDR));
            APIC_BASE_MSR.write(base_addr | (1 << 11)); // Enable the APIC.
            base_addr
        };

        // Enable spurious interrupts.
        {
            let svr_addr = usize::try_from(APIC_BASE_ADDR + 0xf0).unwrap() as *mut u32;
            let val = svr_addr.read_volatile();
            svr_addr.write_volatile(val | 0x100); // Enable spurious interrupts.
        }

        // Set the error handling interrupt vector.
        {
            let lvt_addr = usize::try_from(APIC_BASE_ADDR + 0x370).unwrap() as *mut u32;
            lvt_addr.write_volatile(u32::from(self.error_interrupt_vector.interrupt_num()));
        }
    }

    /// Returns the [`ApicId`] of the calling processor.
    pub fn current_apic_id(&self) -> ApicId {
        current_apic_id()
    }

    /// Returns true if the hardware supports TSC deadline mode.
    pub fn is_tsc_deadline_supported(&self) -> bool {
        self.tsc_deadline_supported
    }

    /// Configures the timer of the local APIC of the current CPU.
    ///
    /// # Panic
    ///
    /// Panics if `TscDeadline` is passed and `is_tsc_deadline_supported` is false.
    /// Panics if the `value_multiplier` is not either one or a power of two.
    pub fn set_local_timer(&self, timer: Timer) {
        unsafe {
            match timer {
                Timer::Disabled => {
                    let addr = usize::try_from(APIC_BASE_ADDR + 0x320).unwrap() as *mut u32;
                    addr.write_volatile(0x00010000);
                }
                Timer::Timer {
                    value,
                    value_multiplier,
                    periodic,
                    vector,
                } => {
                    let divide_config_addr =
                        usize::try_from(APIC_BASE_ADDR + 0x3e0).unwrap() as *mut u32;
                    divide_config_addr.write_volatile(match value_multiplier {
                        1 => 0b1011,
                        2 => 0b0000,
                        4 => 0b0001,
                        8 => 0b0010,
                        16 => 0b0011,
                        32 => 0b1000,
                        64 => 0b1001,
                        128 => 0b1010,
                        _ => panic!(),
                    });

                    let lvt_addr = usize::try_from(APIC_BASE_ADDR + 0x320).unwrap() as *mut u32;
                    let flags = if periodic { 1 << 17 } else { 0 };
                    assert!(vector >= 32);
                    lvt_addr.write_volatile(flags | u32::from(vector));

                    let value_addr = usize::try_from(APIC_BASE_ADDR + 0x380).unwrap() as *mut u32;
                    value_addr.write_volatile(value.get());
                }
                Timer::TscDeadline { threshold, vector } => {
                    assert!(self.tsc_deadline_supported);
                    debug_assert!(is_tsc_deadline_supported());

                    assert!(vector >= 32);
                    let lvt_addr = usize::try_from(APIC_BASE_ADDR + 0x320).unwrap() as *mut u32;
                    let flag = 0b10 << 17;
                    lvt_addr.write_volatile(flag | u32::from(vector));

                    const TIMER_MSR: Msr = Msr::new(0x6e0);
                    TIMER_MSR.write(threshold.get());
                }
            }
        }
    }

    /// Causes the processor with the target APIC ID to wake up.
    ///
    /// # Panic
    ///
    /// Panics if the interrupt vector is inferior to 32.
    ///
    pub fn send_interprocessor_interrupt(&self, target_apic_id: ApicId, vector: u8) {
        assert!(vector >= 32);
        send_ipi_inner(target_apic_id, 0, vector)
    }

    // TODO: documentation
    ///
    ///
    /// # Panic
    ///
    /// Panics if `target_apic_id` is the local APIC.
    ///
    pub fn send_interprocessor_init(&self, target_apic_id: ApicId) {
        assert_ne!(current_apic_id(), target_apic_id);
        send_ipi_inner(target_apic_id, 0b101, 0);
    }

    // TODO: documentation
    ///
    ///
    /// # Panic
    ///
    /// Panics if `target_apic_id` is the local APIC.
    ///
    pub fn send_interprocessor_sipi(&self, target_apic_id: ApicId, boot_fn: *const u8) {
        assert_ne!(current_apic_id(), target_apic_id);

        let boot_fn = boot_fn as usize;
        assert_eq!((boot_fn >> 12) << 12, boot_fn);
        assert!((boot_fn >> 12) <= usize::from(u8::max_value()));
        send_ipi_inner(target_apic_id, 0b110, u8::try_from(boot_fn >> 12).unwrap());
    }
}

/// Address where the APIC registers are mapped.
///
/// While it is possible to remap these registers, this remapping has been made possible by Intel
/// only because of legacy systems and we never use this feature.
const APIC_BASE_ADDR: usize = 0xfee00000;

// Internal implementation of sending an inter-process interrupt.
fn send_ipi_inner(target_apic_id: ApicId, delivery: u8, vector: u8) {
    // Check conformance.
    debug_assert!(delivery <= 0b110);
    debug_assert_ne!(delivery, 0b011);
    debug_assert_ne!(delivery, 0b001);
    debug_assert!(delivery != 0b010 || vector == 0);
    debug_assert!(delivery != 0b101 || vector == 0);

    // TODO: if P6 architecture, then only 4 bits of the target are valid; do we care about that?
    let level_bit = if delivery == 0b101 { 0 } else { 1 << 14 };
    let value_lo = level_bit | (u32::from(delivery) << 8) | u32::from(vector);
    let value_hi = u32::from(target_apic_id.0) << (56 - 32);

    let value_lo_addr = usize::try_from(APIC_BASE_ADDR + 0x300).unwrap() as *mut u32;
    let value_hi_addr = usize::try_from(APIC_BASE_ADDR + 0x310).unwrap() as *mut u32;

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

/// Returns the [`ApicId`] of the calling processor.
fn current_apic_id() -> ApicId {
    unsafe {
        // Note: this is correct because we never modify the local APIC ID.
        let apic_id_addr = usize::try_from(APIC_BASE_ADDR + 0x20).unwrap() as *mut u32;
        let apic_id = u8::try_from(apic_id_addr.read_volatile() >> 24).unwrap();
        ApicId(apic_id)
    }
}

/// Checks in the CPUID whether the APIC is supported.
fn is_apic_supported() -> bool {
    unsafe {
        let cpuid = core::arch::x86_64::__cpuid(0x1);
        cpuid.edx & (1 << 9) != 0
    }
}

/// Checks in the CPUID whether the TSC is supported.
fn is_tsc_supported() -> bool {
    unsafe {
        let cpuid = core::arch::x86_64::__cpuid(0x1);
        cpuid.edx & (1 << 4) != 0
    }
}

/// Checks in the CPUID whether the APIC timer supports TSC deadline mode.
fn is_tsc_deadline_supported() -> bool {
    unsafe {
        let cpuid = core::arch::x86_64::__cpuid(0x1);
        cpuid.ecx & (1 << 24) != 0
    }
}
