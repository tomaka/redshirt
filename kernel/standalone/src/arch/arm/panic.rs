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
    unsafe {
        // TODO: somehow freeze all CPUs?

        init_uart();

        let _ = writeln!(DummyWrite, "Kernel panic!");
        let _ = writeln!(DummyWrite, "{}", panic_info);

        // Freeze forever.
        loop {
            asm!(r#"wfe"#);
        }
    }
}

struct DummyWrite;
impl fmt::Write for DummyWrite {
    fn write_str(&mut self, message: &str) -> fmt::Result {
        for byte in message.as_bytes() {
            write_uart(*byte);
        }
        Ok(())
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
