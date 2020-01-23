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

use alloc::string::String;
use core::fmt::{self, Write};

#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    // TODO: switch back to text mode somehow?

    let mut console = Console::default();

    if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
        let _ = writeln!(console, "panic occurred: {:?}", s);
    } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
        let _ = writeln!(console, "panic occurred: {:?}", s);
    } else if let Some(message) = panic_info.message() {
        let _ = Write::write_fmt(&mut console, *message);
        let _ = writeln!(console, "");
    } else {
        let _ = writeln!(console, "panic occurred");
    }

    if let Some(location) = panic_info.location() {
        let _ = writeln!(
            console,
            "panic occurred in file '{}' at line {}",
            location.file(),
            location.line()
        );
    } else {
        let _ = writeln!(
            console,
            "panic occurred but can't get location information..."
        );
    }

    crate::arch::halt();
}

// State machine for the standard text console.
#[cfg(target_arch = "x86_64")]
#[derive(Default)]
struct Console {
    cursor_x: u8,
    cursor_y: u8,
}

// State machine for the standard text console.
#[cfg(target_arch = "arm")]
struct Console {}

#[cfg(target_arch = "x86_64")]
impl fmt::Write for Console {
    fn write_str(&mut self, message: &str) -> fmt::Result {
        unsafe {
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
                ptr_of(self.cursor_x, self.cursor_y).write_volatile(u16::from(chr) | 0xf00);

                debug_assert!(self.cursor_x < 80);
                self.cursor_x += 1;
                if self.cursor_x == 80 {
                    self.cursor_x = 0;
                    debug_assert!(self.cursor_y < 25);
                    self.cursor_y += 1;
                    if self.cursor_y == 25 {
                        self.cursor_y -= 1;
                        line_up(); // TODO: no?
                    }
                }
            }
        }

        Ok(())
    }
}

// State machine for the standard text console.
#[cfg(target_arch = "arm")]
impl Default for Console {
    fn default() -> Console {
        init_uart();
        Console {}
    }
}

#[cfg(target_arch = "arm")]
impl fmt::Write for Console {
    fn write_str(&mut self, message: &str) -> fmt::Result {
        for byte in message.as_bytes() {
            write_uart(*byte);
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

const GPIO_BASE: usize = 0x3F200000;
const UART0_BASE: usize = 0x3F201000;

fn init_uart() {
    unsafe {
        ((UART0_BASE + 0x30) as *mut u32).write_volatile(0x0);
        ((GPIO_BASE + 0x94) as *mut u32).write_volatile(0x0);
        delay(150);

        ((GPIO_BASE + 0x98) as *mut u32).write_volatile((1 << 14) | (1 << 15));
        delay(150);

        ((GPIO_BASE + 0x98) as *mut u32).write_volatile(0x0);

        ((UART0_BASE + 0x44) as *mut u32).write_volatile(0x7FF);

        ((UART0_BASE + 0x24) as *mut u32).write_volatile(1);
        ((UART0_BASE + 0x28) as *mut u32).write_volatile(40);

        ((UART0_BASE + 0x2C) as *mut u32).write_volatile((1 << 4) | (1 << 5) | (1 << 6));

        ((UART0_BASE + 0x38) as *mut u32).write_volatile(
            (1 << 1) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 10),
        );

        ((UART0_BASE + 0x30) as *mut u32).write_volatile((1 << 0) | (1 << 8) | (1 << 9));
    }
}

fn write_uart(byte: u8) {
    unsafe {
        // Wait for UART to become ready to transmit.
        while (((UART0_BASE + 0x18) as *mut u32).read_volatile() & (1 << 5)) != 0 {}
        ((UART0_BASE + 0x0) as *mut u32).write_volatile(u32::from(byte));
    }
}

fn delay(count: i32) {
    // TODO: asm!("__delay_%=: subs %[count], %[count], #1; bne __delay_%=\n" : "=r"(count): [count]"0"(count) : "cc");
}
