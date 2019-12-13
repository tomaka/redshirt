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

/// State of a device.
pub struct Device {
    base_port: u32,
}

impl Device {
    pub async fn reset(base_port: u32) -> Self {
        // Reads the RESET register and write its value back in order to reset the device.
        nametbd_hardware_interface::port_write_u8(
            base_port + 0x1f,
            nametbd_hardware_interface::port_read_u8(base_port + 0x1f).await
        };

        // Wait for reset to be complete.
        loop {
            let val = nametbd_hardware_interface::port_read_u8(base_port + 0x7).await;
            if (val & 0x80) == 0 { break }
        }

        // Mask interrupts.
        nametbd_hardware_interface::port_write_u8(base_port + 0x7, 0xff);

        // TODO: unfinished

        Device {
            base_port,
        }
    }

    pub fn send_packet(&mut self, packet: &[u8]) {
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
            ops.port_write_u8(self.base_port + 16, byte);
        }

        ops.port_write_u8(self.base_port + 4, 0x40);
        ops.port_write_u8(self.base_port + 5, packet_len_lo);
        ops.port_write_u8(self.base_port + 6, packet_len_hi);

        ops.port_write_u8(self.base_port + 0, (1 << 5) | (1 << 2) | (1 << 1));

        ops.send();
    }
}
