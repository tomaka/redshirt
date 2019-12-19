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

use byteorder::{ByteOrder as _, LittleEndian};
use parity_scale_codec::DecodeAll;
use std::{convert::TryFrom as _, fmt};

fn main() {
    redshirt_syscalls_interface::block_on(async_main());
}

async fn async_main() -> ! {
    redshirt_interface_interface::register_interface(redshirt_stdout_interface::ffi::INTERFACE)
        .await.unwrap();
    init_uart();

    loop {
        let msg = match redshirt_syscalls_interface::next_interface_message().await {
            redshirt_syscalls_interface::InterfaceOrDestroyed::Interface(m) => m,
            redshirt_syscalls_interface::InterfaceOrDestroyed::ProcessDestroyed(_) => continue,
        };
        assert_eq!(msg.interface, redshirt_stdout_interface::ffi::INTERFACE);

        let redshirt_stdout_interface::ffi::StdoutMessage::Message(message) =
            DecodeAll::decode_all(&msg.actual_data).unwrap();       // TODO: don't unwrap
        for byte in message.as_bytes() {
            write_uart(*byte);
        }
    }
}

const GPIO_BASE: u64 = 0x3F200000;
const UART0_BASE: u64 = 0x3F201000;

fn init_uart() {
    unsafe {
        let mut ops = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();

        ops.write_one_u32(UART0_BASE + 0x30, 0x0);
        ops.write_one_u32(GPIO_BASE + 0x94, 0x0);
        delay(150);

        ops.write_one_u32(GPIO_BASE + 0x98, (1 << 14) | (1 << 15));
        delay(150);

        ops.write_one_u32(GPIO_BASE + 0x98, 0x0);

        ops.write_one_u32(UART0_BASE + 0x44, 0x7FF);

        ops.write_one_u32(UART0_BASE + 0x24, 1);
        ops.write_one_u32(UART0_BASE + 0x28, 40);

        ops.write_one_u32(UART0_BASE + 0x2C, (1 << 4) | (1 << 5) | (1 << 6));

        ops.write_one_u32(UART0_BASE + 0x38, 
            (1 << 1) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 10)
        );

        ops.write_one_u32(UART0_BASE + 0x30, (1 << 0) | (1 << 8) | (1 << 9));
        ops.send();
    }
}

async fn write_uart(byte: u8) {
    unsafe {
        // Wait for UART to become ready to transmit.
        loop {
            // TODO: add shortcut in hardware-interface
            let mut read = redshirt_hardware_interface::HardwareOperationsBuilder::new();
            let mut out = [0];
            read.read_u32(UART0_BASE + 0x18, &mut out);
            read.send().await;
            if out[0] & (1 << 5) == 0 { break; }
        }

        redshirt_hardware_interface::write_one_u32(UART0_BASE + 0x0, u32::from(byte));
    }
}

fn delay(count: i32) {
    // TODO: asm!("__delay_%=: subs %[count], %[count], #1; bne __delay_%=\n" : "=r"(count): [count]"0"(count) : "cc");
}
