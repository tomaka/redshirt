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

//! Panic handling code.
//!
//! This panic handler tries to use as little features as possible, in order to maximize the
//! chances of the panic message being displayed. In particular, it doesn't perform any heap
//! allocation.

use core::fmt::{self, Write};
use x86_64::structures::port::PortWrite as _;

// TODO: make panics a bit nicer?

#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    // TODO: switch back to text mode somehow?
    let mut console = Console {
        cursor_x: 0,
        cursor_y: 0,
    };

    let _ = writeln!(console, "Kernel panic!");
    let _ = writeln!(console, "{}", panic_info);
    let _ = writeln!(console, "");

    // Disable the text mode cursor.
    unsafe {
        u8::write_to_port(0x3d4, 0x0a);
        u8::write_to_port(0x3d5, 0x20);
    }

    // Freeze forever.
    loop {
        x86_64::instructions::interrupts::disable();
        x86_64::instructions::hlt();
    }
}

struct Console {
    cursor_x: u8,
    cursor_y: u8,
}

impl fmt::Write for Console {
    fn write_str(&mut self, message: &str) -> fmt::Result {
        unsafe {
            for chr in message.chars() {
                if !chr.is_ascii() {
                    continue;
                }

                // We assume that panic messages are never more than 25 lines and discard
                // everything after.
                if self.cursor_y >= 25 {
                    break;
                }

                if chr == '\n' {
                    while self.cursor_x != 80 {
                        ptr_of(self.cursor_x, self.cursor_y).write_volatile(0);
                        self.cursor_x += 1;
                    }

                    self.cursor_x = 0;
                    self.cursor_y += 1;
                    continue;
                }

                let chr = chr as u8;
                ptr_of(self.cursor_x, self.cursor_y).write_volatile(u16::from(chr) | 0xc00);

                debug_assert!(self.cursor_x < 80);
                self.cursor_x += 1;
                if self.cursor_x == 80 {
                    self.cursor_x = 0;
                    self.cursor_y += 1;
                }
            }
        }

        Ok(())
    }
}

fn ptr_of(x: u8, y: u8) -> *mut u16 {
    assert!(x < 80);
    assert!(y < 25);

    unsafe {
        let offset = isize::from(y) * 80 + isize::from(x);
        (0xb8000 as *mut u16).offset(offset)
    }
}
