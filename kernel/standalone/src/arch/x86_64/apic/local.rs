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
    marker::PhantomData,
    num::NonZeroU64,
    ops::Range,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use futures::prelude::*;
use spin::Mutex;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::port::{PortRead as _, PortWrite as _};

// TODO: "For correct APIC operation, this address space must be mapped to an area of memory that has been designated as strong uncacheable (UC)"

/// Initialized local APIC.
///
/// This represents the APIC of the local CPU, and therefore doesn't implement `Send`.
#[derive(Clone)]
pub struct LocalApicControl {
    /// True if the CPU supports TSC-Deadline mode.
    tsc_deadline_supported: bool,

    /// Marker to make the struct `!Send`.
    no_send_marker: PhantomData<*mut u8>,
}

/// Opaque type representing the APIC ID of a processor.
///
/// Since we never modify the APIC ID of processors, the existence of this struct is a guarantee
/// that a processor with the given ID exists.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ApicId(u8);

/// Makes sure that the current CPU supports an APIC, and initializes it.
///
/// # Panic
///
/// Panics if the CPU doesn't support the APIC.
///
/// # Safety
///
/// Must only be called once, and assumes that no other piece of code reads or writes to the MSR
/// registers related to the local APIC or x2APIC, or the the registers mapped to physical memory.
///
pub unsafe fn init() -> LocalApicControl {
    // TODO: check whether CPUID is supported at all?
    assert!(is_apic_supported());

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

    LocalApicControl {
        tsc_deadline_supported: is_tsc_deadline_supported(),
        no_send_marker: PhantomData,
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
}

impl LocalApicControl {
    /// Returns the [`ApicId`] of the calling processor.
    pub fn current_apic_id(&self) -> ApicId {
        unsafe {
            // Note: this is correct because we never modify the local APIC ID.
            let apic_id_addr = usize::try_from(APIC_BASE_ADDR + 0x20).unwrap() as *mut u32;
            let apic_id = u8::try_from(apic_id_addr.read_volatile() >> 24).unwrap();
            ApicId(apic_id)
        }
    }

    pub fn set_tsc_deadline(&self, value: Option<NonZeroU64>) {
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
    /// Panics if the processor with the `target_apic_id` hasn't started yet. // TODO: <--
    ///
    pub fn send_interprocessor_interrupt(&self, target_apic_id: ApicId, vector: u8) {
        assert!(vector >= 32);
        send_ipi_inner(target_apic_id, 0, vector)
    }

    #[cold]
    pub fn send_interprocessor_init(&self, target_apic_id: ApicId) {
        send_ipi_inner(target_apic_id, 0b101, 0);
    }

    #[cold]
    pub fn send_interprocessor_sipi(&self, target_apic_id: ApicId, boot_fn: *const u8) {
        let boot_fn = boot_fn as usize;
        assert_eq!((boot_fn >> 12) << 12, boot_fn);
        assert!((boot_fn >> 12) <= usize::from(u8::max_value()));
        send_ipi_inner(target_apic_id, 0b110, u8::try_from(boot_fn >> 12).unwrap());
    }
}

/// Address where the APIC registers are mapped.
///
/// While it is possible to remap these registers, this remapping has been made possible because
/// of legacy systems. We never do it.
const APIC_BASE_ADDR: usize = 0xfee00000;

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
