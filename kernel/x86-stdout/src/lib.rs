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

//! Implements the stdout interface by writing in text mode.

#![no_std]

use core::fmt;

/// State machine for the standard text console.
pub struct Console {
    cursor_x: u8,
    cursor_y: u8,
}

impl Console {
    /// Initializes the console.
    ///
    /// # Safety
    ///
    /// - Assumes that we are in text mode and that we are write in the video memory.
    ///
    pub const unsafe fn init() -> Console {
        Console {
            cursor_x: 0,
            cursor_y: 0,
        }
    }

    pub fn clear_screen(&mut self) {
        clear_screen();
    }

    /// Writes a message on the console.
    pub fn write(&mut self, message: &str) {
        unsafe {
            for chr in message.chars() {
                if !chr.is_ascii() {
                    continue;
                }

                if chr == '\r' || chr == '\n' {
                    self.cursor_x = 0;
                    self.cursor_y += 1;
                    if self.cursor_y == 25 {
                        self.cursor_y -= 1;
                        line_up();
                    }
                    continue;
                }

                let chr = chr as u8;
                ptr_of(self.cursor_x, self.cursor_y).write_volatile(u16::from(chr) | 0xf00);

                debug_assert!(self.cursor_x < 80);
                self.cursor_x += 1;
                if self.cursor_x == 80 {
                    self.cursor_x = 0;
                    debug_assert!(self.cursor_y < 25);
                    self.cursor_y += 1;
                    if self.cursor_y == 25 {
                        self.cursor_y -= 1;
                        line_up();
                    }
                }
            }
        }
    }
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write(s);
        Ok(())
    }
}

fn clear_screen() {
    unsafe {
        for y in 0..25 {
            for x in 0..80 {
                ptr_of(x, y).write_volatile(0);
            }
        }
    }
}

fn ptr_of(x: u8, y: u8) -> *mut u16 {
    assert!(x < 80);
    assert!(y < 25);

    unsafe {
        let offset = isize::from((y * 80) + x);
        (0xb8000 as *mut u16).offset(offset)
    }
}

fn line_up() {
    unsafe {
        for y in 1..25 {
            for x in 0..80 {
                let val = ptr_of(x, y).read_volatile();
                ptr_of(x, y - 1).write_volatile(val);
            }
        }

        for x in 0..80 {
            ptr_of(x, 24).write_volatile(0);
        }
    }
}
