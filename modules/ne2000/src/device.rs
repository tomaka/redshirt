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
// The ne2000 has a circular buffer of pages of 256 bytes each. Packets always have to be
// aligned on pages boundaries. Only pages 0x40 to 0x60 are available for us to read/write on.
// We store the .
//
// The device writes received packets to the buffer, and can transmit out packets by reading from
// this buffer.
//
// This buffer isn't available in physical memory. In order to access it from the host (i.e. us),
// we have to use the DMA system of the chip.
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
        redshirt_hardware_interface::port_write_u8(
            base_port + 0x1f,
            redshirt_hardware_interface::port_read_u8(base_port + 0x1f).await
        );

        // Wait for reset to be complete.
        loop {
            let val = redshirt_hardware_interface::port_read_u8(base_port + 7).await;
            if (val & 0x80) != 0 { break }      // TODO: fail after trying too many times
        }

        // Clear interrupts.
        // When an interrupt is triggered, a bit of this register is set to 1. Writing 1 resets it.
        // We reset all.
        redshirt_hardware_interface::port_write_u8(base_port + 7, 0xff);

        // Abort DMA and stop.
        redshirt_hardware_interface::port_write_u8(base_port + 0, (1 << 5) | (1 << 0));

        // Packets with multicast addresses, broadcast addresses and small are all accepted.
        redshirt_hardware_interface::port_write_u8(base_port + 12, (1 << 3) | (1 << 2) | (1 << 1));
        // External lookback. // TODO: is this how we read our MAC?
        redshirt_hardware_interface::port_write_u8(base_port + 13, 1 << 2);

        // TODO: understand
        redshirt_hardware_interface::port_write_u8(base_port + 14, (1 << 6) | (1 << 4) | (1 << 3));

        // Read our MAC address.
        let mac_address: [u8; 6] = {
            let mut buffer = [0; 32];
            dma_read(base_port, &mut buffer, 0).await;
            // TODO: wtf is with these indices? is that correct?
            [buffer[0], buffer[2], buffer[4], buffer[6], buffer[8], buffer[10]]
        };

        redshirt_stdout_interface::stdout(
            format!("MAC: {:x} {:x} {:x} {:x} {:x} {:x}\n", mac_address[0], mac_address[1], mac_address[2], mac_address[3], mac_address[4], mac_address[5])
        );

        // Start page address of the packet to be transmitted.
        redshirt_hardware_interface::port_write_u8(base_port + 4, 0x40);
        // 0x46 to PSTART and BNRY.
        redshirt_hardware_interface::port_write_u8(base_port + 1, 0x4b);
        redshirt_hardware_interface::port_write_u8(base_port + 3, 0x4b);
        // 0x60 to PSTOP (maximum value).
        redshirt_hardware_interface::port_write_u8(base_port + 2, 0x60);
        // Now enable interrupts.
        redshirt_hardware_interface::port_write_u8(base_port + 15, 0x1f);

        // Set to registers page 1. Abort/complete DMA. Stop.
        // (note: found a demo code that sends that twice for an unknown reason)
        redshirt_hardware_interface::port_write_u8(base_port + 0, (1 << 6) | (1 << 5) | (1 << 0));

        // Write to the PAR (Physical Address Registers). Incoming packets are compared with this
        // for acceptance/rejection.
        for n in 0..6u8 {
            redshirt_hardware_interface::port_write_u8(
                base_port + 1 + u32::from(n),
                mac_address[usize::from(n)]
            );
        }

        // Write the MAR (Multicast Address Registers). Filtering bits for multicast packets.
        for n in 8..=15 {
            redshirt_hardware_interface::port_write_u8(base_port + 0 + n, 0xff);
        }

        // Writing the CURR (Current Page Register).
        // TODO: understand
        redshirt_hardware_interface::port_write_u8(base_port + 7, 0x47);
        // Registers to page 0. Abort/complete DMA and start.
        redshirt_hardware_interface::port_write_u8(base_port + 0, (1 << 5) | (1 << 1));
        // Transmit Configuration register. Normal operation.
        redshirt_hardware_interface::port_write_u8(base_port + 13, 0);
        // Receive Configuration register.
        redshirt_hardware_interface::port_write_u8(base_port + 12, (1 << 3) | (1 << 2));

        Device {
            base_port,
            next_packet: 0x47,
            mac_address,
        }
    }

    unsafe fn send_packet(&mut self, packet: &[u8]) {
        dma_write(self.base_port, packet, 0x40);

        let (packet_len_lo, packet_len_hi) = if let Ok(len) = u16::try_from(packet.len()) {
            let len_bytes = len.to_le_bytes();
            (len_bytes[0], len_bytes[1])
        } else {
            panic!()        // TODO:
        };

        let mut ops = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();

        // TODO: check available length


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
        let status = redshirt_hardware_interface::port_read_u8(self.base_port + 7).await;
        // Write back the same status in order to clear the bits and allow further interrupts to
        // happen.
        redshirt_hardware_interface::port_write_u8(self.base_port + 7, status);

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

/// Reads data from the memory of the card.
///
/// Command register must be at page 0.
///
/// # Safety
///
/// Race condition if the same remote memory is at the same time written by something else.
///
async unsafe fn dma_read(base_port: u32, data: &mut [u8], page_start: u8) {
    if data.is_empty() {
        return;
    }

    assert!(usize::from(page_start) + ((data.len() - 1) / 256 + 1) < 0x60);

    let (data_len_lo, data_len_hi) = if let Ok(len) = u16::try_from(data.len()) {
        let len_bytes = len.to_le_bytes();
        (len_bytes[0], len_bytes[1])
    } else {
        panic!()        // TODO:
    };

    let mut ops = redshirt_hardware_interface::HardwareOperationsBuilder::new();

    // DMA remote bytes count set to the length we want to write.
    ops.port_write_u8(base_port + 10, data_len_lo);
    ops.port_write_u8(base_port + 11, data_len_hi);
    // DMA remote start address.
    ops.port_write_u8(base_port + 8, 0);   // A page is 256 bytes, so the low is always 0
    ops.port_write_u8(base_port + 9, page_start);
    // Start & DMA remote read.
    ops.port_write_u8(base_port + 0, (1 << 3) | (1 << 1));

    for byte in data {
        ops.port_read_u8(base_port + 16, byte);
    }

    ops.send().await;
}

/// Writes data to the memory of the card.
///
/// Command register must be at page 0.
///
/// # Safety
///
/// Race condition if the same remote memory is accessed at the same time by something else.
///
unsafe fn dma_write(base_port: u32, data: &[u8], page_start: u8) {
    if data.is_empty() {
        return;
    }

    assert!(page_start >= 0x40 && usize::from(page_start) + ((data.len() - 1) / 256 + 1) < 0x60);

    let (data_len_lo, data_len_hi) = if let Ok(len) = u16::try_from(data.len()) {
        let len_bytes = len.to_le_bytes();
        (len_bytes[0], len_bytes[1])
    } else {
        panic!()        // TODO:
    };

    let mut ops = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();

    // DMA remote bytes count set to the length we want to write.
    ops.port_write_u8(base_port + 10, data_len_lo);
    ops.port_write_u8(base_port + 11, data_len_hi);
    // DMA remote start address.
    ops.port_write_u8(base_port + 8, 0);   // A page is 256 bytes, so the low is always 0
    ops.port_write_u8(base_port + 9, page_start);
    // Remote write + start.
    ops.port_write_u8(base_port + 0, (1 << 4) | (1 << 1));

    // Feed data to the DMA.
    for byte in data {
        ops.port_write_u8(base_port + 16, *byte);
    }

    ops.send();
}
