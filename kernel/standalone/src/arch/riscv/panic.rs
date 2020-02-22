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
    // TODO: somehow freeze all CPUs?

    init();
    let _ = writeln!(DummyWrite, "Kernel panic!");
    let _ = writeln!(DummyWrite, "{}", panic_info);

    unsafe {
        // Freeze forever.
        loop {
            asm!("wfi");
        }
    }
}

fn init() {
    unsafe {
        let prci_hfrosccfg = (0x10008000 as *mut u32).read_volatile();
        (0x10008000 as *mut u32).write_volatile(prci_hfrosccfg | (1 << 30));

        let prci_pllcfg = (0x10008008 as *mut u32).read_volatile();
        (0x10008008 as *mut u32).write_volatile(prci_pllcfg | (1 << 18) | (1 << 17));
        let prci_pllcfg = (0x10008008 as *mut u32).read_volatile();
        (0x10008008 as *mut u32).write_volatile(prci_pllcfg | (1 << 16));

        let prci_hfrosccfg = (0x10008000 as *mut u32).read_volatile();
        (0x10008000 as *mut u32).write_volatile(prci_hfrosccfg & !(1 << 30));


        let gpio_iof_sel = (0x1001203c as *mut u32).read_volatile();
        (0x1001203c as *mut u32).write_volatile(gpio_iof_sel & !0x00030000);

        let gpio_iof_en = (0x10012038 as *mut u32).read_volatile();
        (0x10012038 as *mut u32).write_volatile(gpio_iof_en | 0x00030000);

        (0x10013018 as *mut u32).write_volatile(138);

        let uart_reg_tx_ctrl = (0x10013008 as *mut u32).read_volatile();
        (0x10013008 as *mut u32).write_volatile(uart_reg_tx_ctrl | 1);
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

fn write_uart(byte: u8) {
    unsafe {
        // Wait for UART to become ready to transmit.
        while ((0x10013000 as *mut u32).read_volatile() & 0x80000000) != 0 {}
        (0x10013000 as *mut u32).write_volatile(u32::from(byte));
    }
}
