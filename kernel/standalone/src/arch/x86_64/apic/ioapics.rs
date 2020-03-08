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

//! I/O APICs management.
//!
//! Collection of all the I/O APICs together.

use crate::arch::x86_64::apic::{ioapic, ApicId};

use core::convert::TryFrom as _;
use smallvec::SmallVec;

/// Control over all the I/O APIC.
pub struct IoApicsControl {
    io_apics: SmallVec<[ioapic::IoApicControl; 4]>,
    legacy_redirects: SmallVec<[(u8, u32); 16]>,
}

/// Access to the configuration of an IRQ.
pub struct Irq<'a> {
    inner: ioapic::Irq<'a>,
}

/// Initializes all the I/O APICs.
///
/// # Safety
///
/// The parameters must be valid and refer to a correct list of I/O APICs. This information is
/// normally fetched from the ACPI tables.
///
/// Must only be called once.
///
// TODO: document legacy_redirects
pub unsafe fn init_io_apics(
    list: impl IntoIterator<Item = ioapic::IoApicDescription>,
    legacy_redirects: impl IntoIterator<Item = (u8, u32)>,
) -> IoApicsControl {
    IoApicsControl {
        io_apics: list.into_iter().map(|cfg| ioapic::init_io_apic(cfg)).collect(),
        // TODO: is u8/u32 correct? we convert from the u32 to u8 later, that's bad
        legacy_redirects: legacy_redirects.into_iter().collect(),
    }
}

/// Initializes the I/O APICs from information gathered through the ACPI tables.
///
/// # Safety
///
/// This function is unsafe for the same reasons as [`init_io_apics`]. The parameter is not
/// guaranteed to be authentic.
///
// TODO: meh for this method; depends on external library
pub unsafe fn init_from_acpi(info: &acpi::interrupt::Apic) -> IoApicsControl {
    init_io_apics(
        info.io_apics.iter().map(|io_apic| ioapic::IoApicDescription {
            id: io_apic.id,
            address: usize::try_from(io_apic.address).unwrap(),
            global_system_interrupt_base: u8::try_from(io_apic.global_system_interrupt_base).unwrap(),
        }),
        info.interrupt_source_overrides.iter().map(|ov| (ov.isa_source, ov.global_system_interrupt)),
    )
}

impl IoApicsControl {
    /// Gives access to an object designating the configuration for an ISA IRQ.
    ///
    /// ISA IRQs are considered legacy, but are still used by some hardware.
    pub fn isa_irq(&mut self, isa_irq: u8) -> Option<Irq> {
        let target = self.legacy_redirects.iter()
            .find(|(src, _)| *src == isa_irq)
            .map(|(_, dest)| *dest);

        if let Some(dest) = target {
            self.irq(u8::try_from(dest).unwrap())
        } else {
            self.irq(isa_irq)
        }
    }

    /// Gives access to an object designating the configuration of an IRQ.
    ///
    /// Returns `None` if none of the I/O APICs can handle the given IRQ.
    pub fn irq(&mut self, irq: u8) -> Option<Irq> {
        for io_apic in self.io_apics.iter_mut() {
            if let Some(inner) = io_apic.irq(irq) {
                return Some(Irq {
                    inner,
                });
            }
        }

        None
    }
}

impl<'a> Irq<'a> {
    /// Sets what happens when this IRQ is triggered.
    ///
    /// # Panic
    ///
    /// Panics if `destination_interrupt` is inferior to 32.
    ///
    // TODO: add some kind of assignment system, so that we don't accidentally erase a previous assignment
    pub fn set_destination(&mut self, destination: ApicId, destination_interrupt: u8) {
        self.inner.set_destination(destination, destination_interrupt);
    }
}
