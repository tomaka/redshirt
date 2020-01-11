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

//! Implements the log interface by writing in text mode.

use redshirt_syscalls_interface::{Decode, EncodedMessage};
use std::{convert::TryFrom as _, fmt};

fn main() {
    redshirt_syscalls_interface::block_on(async_main());
}

async fn async_main() -> ! {
    redshirt_interface_interface::register_interface(redshirt_log_interface::ffi::INTERFACE)
        .await.unwrap();

    // TODO: properly initialize VGA? https://gist.github.com/tomaka/8a007d0e3c7064f419b24b044e152c22

    let mut console = unsafe { Console::init() };

    loop {
        let msg = match redshirt_syscalls_interface::next_interface_message().await {
            redshirt_syscalls_interface::InterfaceOrDestroyed::Interface(m) => m,
            redshirt_syscalls_interface::InterfaceOrDestroyed::ProcessDestroyed(_) => continue,
        };
        assert_eq!(msg.interface, redshirt_log_interface::ffi::INTERFACE);
        let message: redshirt_log_interface::ffi::DecodedLogMessage =
            Decode::decode(EncodedMessage(msg.actual_data)).unwrap();       // TODO: don't unwrap
        console.write(&message.message());
        console.write("\n");
    }
}

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
            let mut operation_builder = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();

            for chr in message.chars() {
                if !chr.is_ascii() {
                    continue;
                }

                if chr == '\n' {
                    self.cursor_x = 0;
                    self.cursor_y += 1;
                    if self.cursor_y == 25 {
                        self.cursor_y -= 1;
                        line_up();
                    }
                    continue;
                }

                let chr = chr as u8;
                operation_builder.write(
                    ptr_of(self.cursor_x, self.cursor_y),
                    vec![chr, 0xf]
                );

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

            // Update the VGA cursor to match self.cursor_x and self.cursor_y.
            let cursor_pos = u64::from(self.cursor_y) * 80 + u64::from(self.cursor_x);
            operation_builder.port_write_u8(0x3d4, 0xf);
            operation_builder.port_write_u8(0x3d5, u8::try_from(cursor_pos & 0xff).unwrap());
            operation_builder.port_write_u8(0x3d4, 0xe);
            operation_builder.port_write_u8(0x3d5, u8::try_from((cursor_pos >> 8) & 0xff).unwrap());

            operation_builder.send();
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
        redshirt_hardware_interface::write(
            ptr_of(0, 0),
            (0..(80 * 25 * 2)).map(|_| 0).collect::<Vec<_>>()
        );
    }
}

fn ptr_of(x: u8, y: u8) -> u64 {
    assert!(x < 80);
    assert!(y < 25);

    let offset = 2 * (u64::from(y) * 80 + u64::from(x));
    0xb8000 + offset
}

fn line_up() {
    // TODO:
    /*unsafe {
        for y in 1..25 {
            for x in 0..80 {
                let val = ptr_of(x, y).read_volatile();
                ptr_of(x, y - 1).write_volatile(val);
            }
        }

        for x in 0..80 {
            ptr_of(x, 24).write_volatile(0);
        }
    }*/
}
