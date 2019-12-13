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

use std::convert::TryFrom as _;

/// State of a device.
pub struct Device {
    base_port: u32,
    next_packet: u32,
    mac_address: [u8; 6],
}

impl Device {
    pub async unsafe fn reset(base_port: u32) -> Self {
        // Reads the RESET register and write its value back in order to reset the device.
        nametbd_hardware_interface::port_write_u8(
            base_port + 0x1f,
            nametbd_hardware_interface::port_read_u8(base_port + 0x1f).await
        );

        // Wait for reset to be complete.
        loop {
            let val = nametbd_hardware_interface::port_read_u8(base_port + 7).await;
            if (val & 0x80) == 0 { break }
        }

        // Mask interrupts.
        nametbd_hardware_interface::port_write_u8(base_port + 7, 0xff);

        // lol, I have no idea what this is all doing

        nametbd_hardware_interface::port_write_u8(base_port + 0, 0x21);
        nametbd_hardware_interface::port_write_u8(base_port + 14, 0x58);
        nametbd_hardware_interface::port_write_u8(base_port + 10, 0x21);
        nametbd_hardware_interface::port_write_u8(base_port + 11, 0x0);
        nametbd_hardware_interface::port_write_u8(base_port + 8, 0);
        nametbd_hardware_interface::port_write_u8(base_port + 9, 0);

        nametbd_hardware_interface::port_write_u8(base_port + 0, (1 << 3) | (1 << 1));
        nametbd_hardware_interface::port_write_u8(base_port + 12, 0xe);
        nametbd_hardware_interface::port_write_u8(base_port + 13, 4);

        let mac_address = {
            let mut buffer = [0; 32];
            let mut ops = nametbd_hardware_interface::HardwareOperationsBuilder::new();
            for byte in &mut buffer {
                ops.port_read_u8(base_port + 16, byte);
            }
            ops.send().await;
            [buffer[0], buffer[2], buffer[4], buffer[6], buffer[8], buffer[10]]
        };

        nametbd_hardware_interface::port_write_u8(base_port + 4, 0x40);
        nametbd_hardware_interface::port_write_u8(base_port + 1, 0x46);
        nametbd_hardware_interface::port_write_u8(base_port + 3, 0x46);
        nametbd_hardware_interface::port_write_u8(base_port + 2, 0x60);
        nametbd_hardware_interface::port_write_u8(base_port + 15, 0x1f);
        nametbd_hardware_interface::port_write_u8(base_port + 0, 0x61);
        nametbd_hardware_interface::port_write_u8(base_port + 0, 0x61);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 1, mac_address[0]);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 2, mac_address[1]);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 3, mac_address[2]);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 4, mac_address[3]);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 5, mac_address[4]);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 6, mac_address[5]);
        // TODO: 7?!
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 8, 0xff);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 9, 0xff);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 10, 0xff);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 11, 0xff);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 12, 0xff);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 13, 0xff);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 14, 0xff);
        nametbd_hardware_interface::port_write_u8(base_port + 0 + 15, 0xff);

        nametbd_hardware_interface::port_write_u8(base_port + 14, 0x58);
        nametbd_hardware_interface::port_write_u8(base_port + 7, 0x47);
        nametbd_hardware_interface::port_write_u8(base_port + 0, 0x22);
        nametbd_hardware_interface::port_write_u8(base_port + 13, 0);
        nametbd_hardware_interface::port_write_u8(base_port + 12, 0xc);

        Device {
            base_port,
            next_packet: 0x47,
            mac_address,
        }
    }

    unsafe fn send_packet(&mut self, packet: &[u8]) {
        let mut ops = nametbd_hardware_interface::HardwareWriteOperationsBuilder::new();

        let (packet_len_lo, packet_len_hi) = if let Ok(len) = u16::try_from(packet.len()) {
            let len_bytes = len.to_le_bytes();
            (len_bytes[0], len_bytes[1])
        } else {
            panic!()        // TODO:
        };

        ops.port_write_u8(self.base_port + 10, packet_len_lo);
        ops.port_write_u8(self.base_port + 11, packet_len_hi);
        ops.port_write_u8(self.base_port + 8, 0);
        ops.port_write_u8(self.base_port + 9, 0x40);
        ops.port_write_u8(self.base_port + 0, (1 << 4) | (1 << 1));

        // TODO: check available length
        for byte in packet {
            ops.port_write_u8(self.base_port + 16, *byte);
        }

        ops.port_write_u8(self.base_port + 4, 0x40);
        ops.port_write_u8(self.base_port + 5, packet_len_lo);
        ops.port_write_u8(self.base_port + 6, packet_len_hi);

        ops.port_write_u8(self.base_port + 0, (1 << 5) | (1 << 2) | (1 << 1));

        ops.send();
    }
}
