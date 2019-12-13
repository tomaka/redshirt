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

use parity_scale_codec::DecodeAll;
use std::{convert::TryFrom as _, fmt};

fn main() {
    init_uart();
    nametbd_syscalls_interface::block_on(async_main());
}

async fn async_main() -> ! {
    nametbd_interface_interface::register_interface(nametbd_stdout_interface::ffi::INTERFACE)
        .await.unwrap();

    // TODO: properly initialize VGA? https://gist.github.com/tomaka/8a007d0e3c7064f419b24b044e152c22

    let mut console = unsafe { Console::init() };

    loop {
        let msg = match nametbd_syscalls_interface::next_interface_message().await {
            nametbd_syscalls_interface::InterfaceOrDestroyed::Interface(m) => m,
            nametbd_syscalls_interface::InterfaceOrDestroyed::ProcessDestroyed(_) => continue,
        };
        assert_eq!(msg.interface, nametbd_stdout_interface::ffi::INTERFACE);
        let nametbd_stdout_interface::ffi::StdoutMessage::Message(message) =
            DecodeAll::decode_all(&msg.actual_data).unwrap();       // TODO: don't unwrap
        console.write(&message);
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
