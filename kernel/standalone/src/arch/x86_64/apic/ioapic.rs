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

//! I/O APIC management.
//!
//! The I/O APIC is a replacement for the legacy [PIC](super::pic). Its role is to receive
//! interrupts triggered by the hardware and deliver them to the CPU.

// # Implementation notes.
//
// Reference document for the I/O APIC:
// https://pdos.csail.mit.edu/6.828/2016/readings/ia32/ioapic.pdf
//
// The I/O APIC exposes two memory-mapped registers: one selector, and one window.
// One must write a register number in the selector, then the value of the register is accessible
// through the window.

use crate::arch::x86_64::apic::ApicId;
use core::convert::TryFrom as _;

/// Control over a single I/O APIC.
pub struct IoApicControl {
    /// Pointer to the memory-mapped selection register.
    /// See the implementation notes above.
    io_reg_sel_register: *mut u32,

    /// Pointer to the memory-mapped window register.
    /// See the implementation notes above.
    io_win_register: *mut u32,

    /// First IRQ that this I/O APIC handles. For example if some hardware triggers IRQ 12, and
    /// the value of this field is 9, then how the IRQ is handled will be in field 3.
    global_system_interrupt_base: u8,

    /// Maximum IRQ offset relative to `global_system_interrupt_base` that this I/O APIC
    /// handles.
    maximum_redirection_entry: u8,
}

pub struct IoApicDescription {
    pub id: u8,
    pub address: usize,
    pub global_system_interrupt_base: u8,
}

/// Access to the configuration of an IRQ in this controller.
pub struct Irq<'a> {
    control: &'a mut IoApicControl,
    irq_offset: u8,
}

/// Initializes a single I/O APIC.
///
/// # Safety
///
/// The parameters must be valid and refer to a correct I/O APIC. This information is normally
/// fetched from the ACPI tables.
///
/// Must only be called once per I/O APIC.
///
pub unsafe fn init_io_apic(config: IoApicDescription) -> IoApicControl {
    let io_reg_sel_register = config.address as *mut u32;
    let io_win_register = config.address.checked_add(0x10).unwrap() as *mut u32;

    let maximum_redirection_entry = {
        io_reg_sel_register.write_volatile(0x1);
        let io_apic_ver = io_win_register.read_volatile();
        u8::try_from((io_apic_ver >> 16) & 0xff).unwrap()
    };

    let io_apic_control = IoApicControl {
        io_reg_sel_register,
        io_win_register,
        global_system_interrupt_base: config.global_system_interrupt_base,
        maximum_redirection_entry,
    };

    // Basic sanity check.
    assert_eq!(config.id, u8::try_from((io_apic_control.read_register(0) >> 24) & 0b1111).unwrap());

    io_apic_control
}

impl IoApicControl {
    /// Gives access to an object designating the configuration of an IRQ in this I/O APIC.
    ///
    /// Returns `None` if this I/O APIC doesn't handle the given IRQ.
    pub fn irq(&mut self, irq: u8) -> Option<Irq> {
        let irq_offset = irq.checked_sub(self.global_system_interrupt_base)?;

        if irq_offset > self.maximum_redirection_entry {
            return None;
        }

        Some(Irq {
            control: self,
            irq_offset,
        })
    }

    /// Modifies the IRQ definition.
    ///
    /// Keep in mind that `irq_offset` is relative to `self.global_system_interrupt_base`.
    // TODO: do we need to be able to set Edge/Level and that kind of stuff?
    fn set_irq(&mut self, irq_offset: u8, destination: ApicId, destination_interrupt: u8) {
        assert!(irq_offset <= self.maximum_redirection_entry);
        assert!(destination_interrupt >= 32);

        assert!(destination.get() < (1 << 4));  // Only 4bits are valid.
        let value = (u64::from(destination.get()) << 56) | u64::from(destination_interrupt);

        let register_base = 0x10u8.checked_add(irq_offset.checked_mul(2).unwrap()).unwrap();

        // Disable interrupts while we're writing the registers, in order to avoid any IRQ
        // happening in-between the two writes.
        let interrupts_enabled = x86_64::instructions::interrupts::are_enabled();
        x86_64::instructions::interrupts::disable();

        self.write_register(register_base, u32::try_from(value & 0xffffffff).unwrap());
        self.write_register(register_base + 1, u32::try_from(value >> 32).unwrap());

        if interrupts_enabled {
            x86_64::instructions::interrupts::enable();
        }
    }

    unsafe fn read_register(&mut self, reg_num: u8) -> u32 {
        self.io_reg_sel_register.write_volatile(u32::from(reg_num));
        self.io_win_register.read_volatile()
    }

    unsafe fn write_register(&mut self, reg_num: u8, value: u32) {
        self.io_reg_sel_register.write_volatile(u32::from(reg_num));
        self.io_win_register.write_volatile(value)
    }
}

impl<'a> Irq<'a> {
    /// Sets what happens when this IRQ is triggered.
    ///
    /// # Panic
    ///
    /// Panics if `destination_interrupt` is inferior to 32.
    ///
    pub fn set_destination(&mut self, destination: ApicId, destination_interrupt: u8) {
        self.control.set_irq(self.irq_offset, destination, destination_interrupt)
    }
}
