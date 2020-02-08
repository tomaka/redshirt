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

//! Programmable Interrupt Timer (PIT)
//!
//! The PIT is a chip that allows raising an Interrupt ReQuest (IRQ) after a certain time has
//! elapsed. This IRQ is propagated to [the I/O APIC], which then delivers an interrupt to one
//! or more processors.
//!
//! In order to determine which IRQ is raised, one need to look at the interrupt source overrides
//! of the ACPI tables for an entry corresponding to ISA IRQ 0.

use x86_64::structures::port::PortWrite as _;

pub struct PitControl {

}

///
///
/// There should only ever be one [`PitControl`] alive at any given point in time. Creating
/// multiple [`PitControl`] is safe, but will lead to logic error.
pub fn init_pit() -> PitControl {
    PitControl {}
}

impl PitControl {

}

/// Instructs the PIT to trigger an IRQ0 after the specified number of ticks have elapsed.
/// The tick frequency is approximately equal to 1.193182 MHz.
fn channel0_one_shot(ticks: u16) {
    unsafe {
        // Set channel 0 to "interrupt on terminal count" mode and prepare for writing the value.
        u8::write_to_port(0x43, 0b00110000);

        let bytes = ticks.to_le_bytes();
        u8::write_to_port(0x40, bytes[0]);
        u8::write_to_port(0x40, bytes[1]);
    }
}
