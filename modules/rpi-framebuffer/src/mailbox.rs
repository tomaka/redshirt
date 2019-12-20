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

// TODO: more docs at https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interface

use std::convert::TryFrom as _;

/// Message to write to the mailbox, or read from the mailbox.
pub struct Message {
    pub channel: u8,
    pub data: u32,
}

const BASE_IO_PERIPH: u64 = 0x3f000000; // 0x20000000 for raspi 1
const MAILBOX_BASE: u64 = BASE_IO_PERIPH + 0xb880;

/// Reads one message from the mailbox.
pub async fn read_mailbox() -> Message {
    unsafe {
        // Wait for status register to indicate a message.
        loop {
            // TODO: add shortcut in hardware-interface
            let mut read = redshirt_hardware_interface::HardwareOperationsBuilder::new();
            let mut out = [0];
            read.read_u32(MAILBOX_BASE + 0x18, &mut out);
            read.send().await;
            if out[0] & (1 << 30) == 0 { break; }
        }

        let mut read = redshirt_hardware_interface::HardwareOperationsBuilder::new();
        let mut out = [0];
        read.read_u32(MAILBOX_BASE + 0x0, &mut out);
        read.send().await;

        let channel = u8::try_from(out[0] & 0xf).unwrap();
        let data = out[0] >> 4;
        Message {
            channel,
            data,
        }
    }
}

/// Writes one message from the mailbox.
pub async fn write_mailbox(message: Message) {
    unsafe {
        // Wait for status register to indicate a message.
        loop {
            // TODO: add shortcut in hardware-interface
            let mut read = redshirt_hardware_interface::HardwareOperationsBuilder::new();
            let mut out = [0];
            read.read_u32(MAILBOX_BASE + 0x18, &mut out);
            read.send().await;
            if out[0] & (1 << 31) == 0 { break; }
        }

        assert!(message.data < (1 << 28));
        let message: u32 = u32::from(message.channel) | (message.data << 4);
        redshirt_hardware_interface::write_one_u32(MAILBOX_BASE + 0x20, message);
    }
}
