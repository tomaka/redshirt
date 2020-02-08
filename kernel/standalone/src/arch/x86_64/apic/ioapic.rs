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

use core::convert::TryFrom as _;

pub struct IoApicControl {
    io_reg_sel_register: *mut u32,
    io_win_register: *mut u32,
    global_system_interrupt_base: u8,
    maximum_redirection_entry: u8,
}

pub unsafe fn init_io_apic(id: u8, address: usize, global_system_interrupt_base: u8) -> IoApicControl {
    let io_reg_sel_register = address as *mut u32;
    let io_win_register = address.checked_add(0x10).unwrap() as *mut u32;

    let maximum_redirection_entry = {
        io_reg_sel_register.write_volatile(0x1);
        let io_apic_ver = io_win_register.read_volatile();
        u8::try_from((io_apic_ver >> 16) & 0xff).unwrap()
    };

    IoApicControl {
        io_reg_sel_register,
        io_win_register,
        global_system_interrupt_base,
        maximum_redirection_entry,
    }
}

impl IoApicControl {
    unsafe fn read_register(&mut self, reg_num: u8) -> u32 {
        self.io_reg_sel_register.write_volatile(u32::from(reg_num));
        self.io_win_register.read_volatile()
    }

    unsafe fn write_register(&mut self, reg_num: u8, value: u32) {
        self.io_reg_sel_register.write_volatile(u32::from(reg_num));
        self.io_win_register.write_volatile(value)
    }
}
