// Copyright (C) 2019-2021  Pierre Krieger
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

use x86_64::structures::port::PortWrite as _;

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
///
/// # Safety
///
/// This function is not thread-safe. It must only be called once simultaneously and while nothing
/// else is accessing the PIC.
///
pub unsafe fn init_and_disable_pic() {
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
