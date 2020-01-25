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

//! Implements the log interface by writing in text mode.

use redshirt_log_interface::ffi;
use redshirt_syscalls::{Decode, EncodedMessage};
use std::{convert::TryFrom as _, mem};

fn main() {
    std::panic::set_hook(Box::new(move |panic_info| {
        redshirt_syscalls::block_on(async move {
            // TODO: make this code alloc-free?

            let mut console = Console {
                cursor_x: 0,
                cursor_y: 0,
                screen_width: 80,
                screen_height: 25,
                ops_buffer: redshirt_hardware_interface::HardwareWriteOperationsBuilder::new(),
            };

            console.write("x86-log has panicked\n", 0xc).await;
            console.write(&panic_info.to_string(), 0xc).await;
            console.flush();
        });
    }));

    redshirt_syscalls::block_on(async_main());
}

async fn async_main() -> ! {
    redshirt_interface_interface::register_interface(ffi::INTERFACE)
        .await
        .unwrap();

    // TODO: properly initialize VGA? https://gist.github.com/tomaka/8a007d0e3c7064f419b24b044e152c22

    let mut console = Console {
        cursor_x: 0,
        cursor_y: 0,
        screen_width: 80,
        screen_height: 25,
        ops_buffer: redshirt_hardware_interface::HardwareWriteOperationsBuilder::new(),
    };

    console.clear_screen();
    console.flush();

    loop {
        let msg = match redshirt_syscalls::next_interface_message().await {
            redshirt_syscalls::DecodedInterfaceOrDestroyed::Interface(m) => m,
            redshirt_syscalls::DecodedInterfaceOrDestroyed::ProcessDestroyed(_) => continue,
        };
        assert_eq!(msg.interface, ffi::INTERFACE);

        if let Ok(message) = ffi::DecodedLogMessage::decode(msg.actual_data) {
            let level = match message.level() {
                ffi::Level::Error => "ERR ",
                ffi::Level::Warn => "WARN",
                ffi::Level::Info => "INFO",
                ffi::Level::Debug => "DEBG",
                ffi::Level::Trace => "TRCE",
            };

            console.write("[", 0x8).await;
            console.write(&format!("{:?}", msg.emitter_pid), 0x8).await;
            console.write("] [", 0x8).await;
            console.write(level, 0x8).await;
            console.write("] ", 0x8).await;
            console.write(&message.message(), 0xf).await;
            console.write("\n", 0xf).await;
        } else {
            console.write("[", 0x8).await;
            console.write(&format!("{:?}", msg.emitter_pid), 0x8).await;
            console.write("] Bad log message\n", 0x8).await;
        }

        console.flush();
    }
}

/// State machine for the standard text console.
struct Console {
    cursor_x: u8,
    cursor_y: u8,
    /// Width of the screen in number of characters.
    screen_width: u8,
    /// Height of the screen in number of characters.
    screen_height: u8,
    ops_buffer: redshirt_hardware_interface::HardwareWriteOperationsBuilder,
}

impl Console {
    fn clear_screen(&mut self) {
        unsafe {
            self.ops_buffer.write(
                self.ptr_of(0, 0),
                (0..(self.screen_width * self.screen_height * 2))
                    .map(|_| 0)
                    .collect::<Vec<_>>(),
            );

            self.cursor_x = 0;
            self.cursor_y = 0;
        }
    }

    fn flush(&mut self) {
        self.update_cursor();

        let new_ops = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();
        mem::replace(&mut self.ops_buffer, new_ops).send();
    }

    fn ptr_of(&self, x: u8, y: u8) -> u64 {
        assert!(x < self.screen_width);
        assert!(y < self.screen_height);

        let offset = 2 * (u64::from(y) * u64::from(self.screen_width) + u64::from(x));
        0xb8000 + offset
    }

    /// Writes a message on the console.
    async fn write(&mut self, message: &str, color: u8) {
        unsafe {
            for chr in message.chars() {
                if !chr.is_ascii() {
                    continue;
                }

                if chr == '\n' {
                    self.cursor_x = 0;
                    self.cursor_y += 1;
                    if self.cursor_y == self.screen_height {
                        self.line_up().await;
                    }
                    continue;
                }

                // We checked `chr.is_ascii()` above
                let chr = u8::try_from(u32::from(chr)).unwrap();

                self.ops_buffer
                    .write(self.ptr_of(self.cursor_x, self.cursor_y), vec![chr, color]);

                debug_assert!(self.cursor_x < self.screen_width);
                self.cursor_x += 1;
                if self.cursor_x == self.screen_width {
                    self.cursor_x = 0;
                    debug_assert!(self.cursor_y < self.screen_height);
                    self.cursor_y += 1;
                    if self.cursor_y == self.screen_height {
                        self.line_up().await;
                    }
                }
            }
        }
    }

    fn update_cursor(&mut self) {
        unsafe {
            let cursor_pos =
                u64::from(self.cursor_y) * u64::from(self.screen_width) + u64::from(self.cursor_x);
            self.ops_buffer.port_write_u8(0x3d4, 0xf);
            self.ops_buffer
                .port_write_u8(0x3d5, u8::try_from(cursor_pos & 0xff).unwrap());
            self.ops_buffer.port_write_u8(0x3d4, 0xe);
            self.ops_buffer
                .port_write_u8(0x3d5, u8::try_from((cursor_pos >> 8) & 0xff).unwrap());
        }
    }

    async fn line_up(&mut self) {
        unsafe {
            self.flush();

            let mut fb_content =
                vec![0; 2 * usize::from(self.screen_width) * (usize::from(self.screen_height) - 1)];

            let mut read_ops = redshirt_hardware_interface::HardwareOperationsBuilder::new();
            read_ops.read(self.ptr_of(0, 1), &mut fb_content);
            read_ops.send().await;

            self.ops_buffer.write(self.ptr_of(0, 0), fb_content);
            self.ops_buffer.write(
                self.ptr_of(0, self.screen_height - 1),
                vec![0; 2 * usize::from(self.screen_width)],
            );

            self.cursor_y -= 1;
        }
    }
}
