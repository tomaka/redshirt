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

//! Implements the log interface by writing on the UART.

use redshirt_log_interface::ffi;
use redshirt_syscalls::{Decode, EncodedMessage};
use std::{convert::TryFrom as _, fmt, sync::atomic};

fn main() {
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() -> ! {
    redshirt_interface_interface::register_interface(ffi::INTERFACE)
        .await
        .unwrap();
    init_uart();

    loop {
        let msg = match redshirt_syscalls::next_interface_message().await {
            redshirt_syscalls::DecodedInterfaceOrDestroyed::Interface(m) => m,
            redshirt_syscalls::DecodedInterfaceOrDestroyed::ProcessDestroyed(_) => continue,
        };

        assert_eq!(msg.interface, ffi::INTERFACE);

        if let Ok(message) = ffi::DecodedLogMessage::decode(msg.actual_data) {
            let level = match message.level() {
                ffi::Level::Error => b"ERR ",
                ffi::Level::Warn => b"WARN",
                ffi::Level::Info => b"INFO",
                ffi::Level::Debug => b"DEBG",
                ffi::Level::Trace => b"TRCE",
            };

            write_utf8_bytes(b"[").await;
            write_utf8_bytes(format!("{:?}", msg.emitter_pid).as_bytes()).await;
            write_utf8_bytes(b"] [").await;
            write_utf8_bytes(level).await;
            write_utf8_bytes(b"] ").await;
            write_untrusted_str(message.message()).await;
            write_utf8_bytes(b"\n").await;
        } else {
            write_utf8_bytes(b"[").await;
            write_untrusted_str(&format!("{:?}", msg.emitter_pid)).await;
            write_utf8_bytes(b"] Bad log message\n").await;
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
        for _ in 0..150 {
            // TODO: does this actually do what it looks like it's doing?
            atomic::spin_loop_hint();
        }

        ops.write_one_u32(GPIO_BASE + 0x98, (1 << 14) | (1 << 15));
        for _ in 0..150 {
            // TODO: does this actually do what it looks like it's doing?
            atomic::spin_loop_hint();
        }

        ops.write_one_u32(GPIO_BASE + 0x98, 0x0);

        ops.write_one_u32(UART0_BASE + 0x44, 0x7FF);

        ops.write_one_u32(UART0_BASE + 0x24, 1);
        ops.write_one_u32(UART0_BASE + 0x28, 40);

        ops.write_one_u32(UART0_BASE + 0x2C, (1 << 4) | (1 << 5) | (1 << 6));

        ops.write_one_u32(
            UART0_BASE + 0x38,
            (1 << 1) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 10),
        );

        ops.write_one_u32(UART0_BASE + 0x30, (1 << 0) | (1 << 8) | (1 << 9));
        ops.send();
    }
}

/// Writes a string after stripping down all undesired control characters.
async fn write_untrusted_str(s: &str) {
    for chr in s.chars() {
        if chr.is_control() {
            let s = chr.escape_unicode().collect::<String>();
            write_utf8_bytes(s.as_bytes()).await;
        } else {
            let mut utf8 = [0; 4];
            let len = chr.encode_utf8(&mut utf8[..]).len();
            write_utf8_bytes(&utf8[..len]).await;
        }
    }
}

/// Writes a list of bytes.
async fn write_utf8_bytes(bytes: &[u8]) {
    for byte in bytes {
        write_byte(*byte).await;
    }
}

/// Writes a single byte.
async fn write_byte(byte: u8) {
    unsafe {
        // Wait for UART to become ready to transmit.
        loop {
            let val = redshirt_hardware_interface::read_one_u32(UART0_BASE + 0x18).await;
            if val & (1 << 5) == 0 {
                break;
            }
        }

        redshirt_hardware_interface::write_one_u32(UART0_BASE + 0x0, u32::from(byte));
    }
}
