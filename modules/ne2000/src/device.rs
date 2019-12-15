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
//
// # Device overview
//
// The ne2000 has a circular buffer of 96 pages of 256 bytes each. Packets always have to be
// aligned on pages boundaries. Only the last 48 pages are available for us to read/write on.
//
// The device writes received packets to the buffer, and can transmit out packets by reading from
// this buffer.
//
// In order to access this buffer from the host (i.e. us), we have to use the DMA system of the
// chip.
//
// # Implementation note
//
// We always maintain the device in started mode and with registers page 0.
// All methods require `&mut self`, guaranteed the lack of race conditions.
// If there's a need to change the registers page, it must be set back to 0 afterwards.
//
pub struct Device {
    base_port: u32,
    next_packet: u32,
    mac_address: [u8; 6],
}

impl Device {
    /// Assumes that an ne2000 device is mapped starting at `base_port` and reinitializes it
    /// to a starting state.
    pub async unsafe fn reset(base_port: u32) -> Self {
        // Reads the RESET register and write its value back in order to reset the device.
        nametbd_hardware_interface::port_write_u8(
            base_port + 0x1f,
            nametbd_hardware_interface::port_read_u8(base_port + 0x1f).await
        );

        // Wait for reset to be complete.
        loop {
            let val = nametbd_hardware_interface::port_read_u8(base_port + 7).await;
            if (val & 0x80) != 0 { break }      // TODO: fail after trying too many times
        }

        // Clear interrupts.
        // When an interrupt is triggered, a bit of this register is set to 1. Writing 1 resets it.
        // We reset all.
        nametbd_hardware_interface::port_write_u8(base_port + 7, 0xff);

        // Abort DMA and stop.
        nametbd_hardware_interface::port_write_u8(base_port + 0, (1 << 5) | (1 << 0));

        // Packets with multicast addresses, broadcast addresses and small are all accepted.
        nametbd_hardware_interface::port_write_u8(base_port + 12, (1 << 3) | (1 << 2) | (1 << 1));
        // External lookback. // TODO: is this how we read our MAC?
        nametbd_hardware_interface::port_write_u8(base_port + 13, 1 << 2);

        // TODO: understand
        nametbd_hardware_interface::port_write_u8(base_port + 14, (1 << 6) | (1 << 4) | (1 << 3));

        // Remote byte count set to 32 in order to prepare for reading the MAC address.
        nametbd_hardware_interface::port_write_u8(base_port + 10, 32);
        nametbd_hardware_interface::port_write_u8(base_port + 11, 0);
        // Set DMA to 0.
        nametbd_hardware_interface::port_write_u8(base_port + 8, 0);
        nametbd_hardware_interface::port_write_u8(base_port + 9, 0);
        // Start & DMA remote read.
        nametbd_hardware_interface::port_write_u8(base_port + 0, (1 << 3) | (1 << 1));

        // Read our MAC address.
        let mac_address: [u8; 6] = {
            let mut buffer = [0; 32];
            let mut ops = nametbd_hardware_interface::HardwareOperationsBuilder::new();
            for byte in &mut buffer {
                ops.port_read_u8(base_port + 16, byte);
            }
            ops.send().await;
            // TODO: wtf is with these indices? is that correct?
            [buffer[0], buffer[2], buffer[4], buffer[6], buffer[8], buffer[10]]
        };

        nametbd_stdout_interface::stdout(
            format!("MAC: {:x} {:x} {:x} {:x} {:x} {:x}\n", mac_address[0], mac_address[1], mac_address[2], mac_address[3], mac_address[4], mac_address[5])
        );

        // Start page address of the packet to be transmitted.
        nametbd_hardware_interface::port_write_u8(base_port + 4, 0x40);
        // 0x46 to PSTART and BNRY.
        nametbd_hardware_interface::port_write_u8(base_port + 1, 0x46);
        nametbd_hardware_interface::port_write_u8(base_port + 3, 0x46);
        // 0x60 to PSTOP (maximum value).
        nametbd_hardware_interface::port_write_u8(base_port + 2, 0x60);
        // Now enable interrupts.
        nametbd_hardware_interface::port_write_u8(base_port + 15, 0x1f);

        // Set to registers page 1. Abort/complete DMA. Stop.
        // (note: found a demo code that sends that twice for an unknown reason)
        nametbd_hardware_interface::port_write_u8(base_port + 0, (1 << 6) | (1 << 5) | (1 << 0));

        // Write to the PAR (Physical Address Registers). Incoming packets are compared with this
        // for acceptance/rejection.
        for n in 0..6u8 {
            nametbd_hardware_interface::port_write_u8(
                base_port + 1 + u32::from(n),
                mac_address[usize::from(n)]
            );
        }

        // Write the MAR (Multicast Address Registers). Filtering bits for multicast packets.
        for n in 8..=15 {
            nametbd_hardware_interface::port_write_u8(base_port + 0 + n, 0xff);
        }

        // Writing the CURR (Current Page Register).
        // TODO: understand
        nametbd_hardware_interface::port_write_u8(base_port + 7, 0x47);
        // Registers to page 0. Abort/complete DMA and start.
        nametbd_hardware_interface::port_write_u8(base_port + 0, (1 << 5) | (1 << 1));
        // Transmit Configuration register. Normal operation.
        nametbd_hardware_interface::port_write_u8(base_port + 13, 0);
        // Receive Configuration register.
        nametbd_hardware_interface::port_write_u8(base_port + 12, (1 << 3) | (1 << 2));

        Device {
            base_port,
            next_packet: 0x47,
            mac_address,
        }
    }

    unsafe fn send_packet(&mut self, packet: &[u8]) {
        let (packet_len_lo, packet_len_hi) = if let Ok(len) = u16::try_from(packet.len()) {
            let len_bytes = len.to_le_bytes();
            (len_bytes[0], len_bytes[1])
        } else {
            panic!()        // TODO:
        };

        let mut ops = nametbd_hardware_interface::HardwareWriteOperationsBuilder::new();

        // TODO: check available length

        // DMA remote bytes count set to the length we want to write.
        ops.port_write_u8(self.base_port + 10, packet_len_lo);
        ops.port_write_u8(self.base_port + 11, packet_len_hi);
        // DMA remote start address.
        ops.port_write_u8(self.base_port + 8, 0);
        ops.port_write_u8(self.base_port + 9, 0x40);
        // Remote write + start.
        ops.port_write_u8(self.base_port + 0, (1 << 4) | (1 << 1));

        // Feed data to the DMA.
        for byte in packet {
            ops.port_write_u8(self.base_port + 16, *byte);
        }

        // Set transmit page start to address where we wrote.
        ops.port_write_u8(self.base_port + 4, 0x40);
        // Length to transmit.
        ops.port_write_u8(self.base_port + 5, packet_len_lo);
        ops.port_write_u8(self.base_port + 6, packet_len_hi);

        // Abort/complete DMA + Transmit packet + Start.
        ops.port_write_u8(self.base_port + 0, (1 << 5) | (1 << 2) | (1 << 1));

        ops.send();

        // TODO: wait until transmitted
    }

    pub async unsafe fn on_interrupt(&mut self) {
        // Read the ISR (Interrupt Status Register) to determine why an interrupt has been raised.
        let status = nametbd_hardware_interface::port_read_u8(self.base_port + 7).await;
        // Write back the same status in order to clear the bits and allow further interrupts to
        // happen.
        nametbd_hardware_interface::port_write_u8(self.base_port + 7, status);

        if (status & (1 << 0)) != 0 {
            // Packet received with no error.
            // TODO: read packet

        } else if (status & (1 << 1)) != 0 || (status & (1 << 3)) != 0 {
            // Packet transmission successful or aborted. We don't treat the "aborted" situation
            // differently than the successful situation.
            // TODO: inform of successful packet transfer
        }
    }
}
