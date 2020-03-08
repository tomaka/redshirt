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

use alloc::sync::Arc;
use core::{
    convert::TryFrom as _,
    num::{NonZeroU32, NonZeroU64},
};
use x86_64::registers::model_specific::Msr;

// TODO: "For correct APIC operation, this address space must be mapped to an area of memory that has been designated as strong uncacheable (UC)"

/// Represents the local APICs of all CPUs.
pub struct LocalApicsControl {
    /// True if the CPUs support TSC-Deadline mode.
    tsc_deadline_supported: bool,
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
/// to the MSR registers related to the local APIC or x2APIC, or the the registers mapped to
/// physical memory.
///
pub unsafe fn init() -> LocalApicsControl {
    // TODO: check whether CPUID is supported at all?

    // We don't support platforms without an APIC.
    assert!(is_apic_supported());

    LocalApicsControl {
        tsc_deadline_supported: is_tsc_deadline_supported(),
    }
}

impl ApicId {
    /// Builds an [`ApicId`] from a raw identifier without checking the value.
    ///
    /// # Safety
    ///
    /// There must be a processor with the given APIC ID.
    ///
    pub unsafe fn from_unchecked(val: u8) -> Self {
        ApicId(val)
    }

    /// Returns the integer value of this ID.
    pub fn get(&self) -> u8 {
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
    unsafe fn init_local(&self) {
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
    }

    /// Returns the [`ApicId`] of the calling processor.
    pub fn current_apic_id(&self) -> ApicId {
        current_apic_id()
    }

    pub fn is_tsc_deadline_supported(&self) -> bool {
        self.tsc_deadline_supported
    }

    // TODO: bad API
    pub fn enable_local_timer_interrupt_tsc_deadline(&self, vector: u8) {
        unsafe {
            assert!(self.tsc_deadline_supported);
            assert!(vector >= 32);
            let addr = usize::try_from(APIC_BASE_ADDR + 0x320).unwrap() as *mut u32;
            let flag = 0b10 << 17;
            addr.write_volatile(flag | u32::from(vector));

            // TODO: hack
            let divide_config_addr = usize::try_from(APIC_BASE_ADDR + 0x3e0).unwrap() as *mut u32;
            divide_config_addr.write_volatile(0b1010); // Divide by 128
        }
    }

    // TODO: bad API
    pub fn enable_local_timer_interrupt(&self, periodic: bool, vector: u8) {
        unsafe {
            assert!(!self.tsc_deadline_supported);
            assert!(vector >= 32);
            let addr = usize::try_from(APIC_BASE_ADDR + 0x320).unwrap() as *mut u32;
            let periodic = if periodic { 1 << 17 } else { 0 };
            addr.write_volatile(periodic | u32::from(vector));

            // TODO: hack
            let divide_config_addr = usize::try_from(APIC_BASE_ADDR + 0x3e0).unwrap() as *mut u32;
            divide_config_addr.write_volatile(0b1010); // Divide by 128
        }
    }

    // TODO: bad API
    pub fn set_local_timer_value(&self, value: Option<NonZeroU32>) {
        unsafe {
            assert!(!self.tsc_deadline_supported);
            let addr = usize::try_from(APIC_BASE_ADDR + 0x380).unwrap() as *mut u32;
            let value = value.map(|v| v.get()).unwrap_or(0);
            addr.write_volatile(value);
        }
    }

    // TODO: bad API
    pub fn disable_local_timer_interrup(&self) {
        unsafe {
            let addr = usize::try_from(APIC_BASE_ADDR + 0x320).unwrap() as *mut u32;
            addr.write_volatile(0x00010000);
        }
    }

    // TODO: bad API
    pub fn set_local_tsc_deadline(&self, value: Option<NonZeroU64>) {
        unsafe {
            assert!(self.tsc_deadline_supported);

            const TIMER_MSR: Msr = Msr::new(0x6e0);
            TIMER_MSR.write(value.map(|v| v.get()).unwrap_or(0));
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
    let level_bit = if delivery == 0b101 { 0 } else { 1 << 15 };
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

/// Checks in the CPUID whether the APIC timer supports TSC deadline mode.
fn is_tsc_deadline_supported() -> bool {
    unsafe {
        let cpuid = core::arch::x86_64::__cpuid(0x1);
        cpuid.ecx & (1 << 24) != 0
    }
}
