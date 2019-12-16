// Copyright (C) 2019  Pierre Krieger
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

use core::convert::TryFrom as _;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::port::{PortRead as _, PortWrite as _};

mod interrupts;

pub unsafe fn init() {
    // Remap and disable the PIC.
    //
    // The PIC (Programmable Interrupt Controller) is the old chip responsible for triggering
    // on the CPU interrupts coming from the hardware.
    //
    // Because of poor design decisions, it will by default trigger interrupts 0 to 32 on the CPU,
    // which are normally reserved for software-related concerns. For example, the timer will by
    // default trigger interrupt 8, which is also the double fault exception handler.
    //
    // In order to solve this issue, one has to reconfigure the PIC in order to make it trigger
    // interrupts between 32 and 48 rather than 0 to 16.
    //
    // Note that this code disables the PIC altogether. Despite the PIC being disabled, it is
    // still possible to receive spurious interrupts. Hence the remapping.
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

    interrupts::init();
}

pub unsafe fn write_port_u8(port: u32, data: u8) {
    if let Ok(port) = u16::try_from(port) {
        u8::write_to_port(port, data);
    }
}

pub unsafe fn write_port_u16(port: u32, data: u16) {
    if let Ok(port) = u16::try_from(port) {
        u16::write_to_port(port, data);
    }
}

pub unsafe fn write_port_u32(port: u32, data: u32) {
    if let Ok(port) = u16::try_from(port) {
        u32::write_to_port(port, data);
    }
}

pub unsafe fn read_port_u8(port: u32) -> u8 {
    if let Ok(port) = u16::try_from(port) {
        u8::read_from_port(port)
    } else {
        0
    }
}

pub unsafe fn read_port_u16(port: u32) -> u16 {
    if let Ok(port) = u16::try_from(port) {
        u16::read_from_port(port)
    } else {
        0
    }
}

pub unsafe fn read_port_u32(port: u32) -> u32 {
    if let Ok(port) = u16::try_from(port) {
        u32::read_from_port(port)
    } else {
        0
    }
}
