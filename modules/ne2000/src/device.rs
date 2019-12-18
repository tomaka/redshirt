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

use core::{convert::TryFrom as _, fmt, ops::Range};

/// State of a device.
//
// # Device overview
//
// The ne2000 has a circular buffer of pages of 256 bytes each. An Ethernet packet can occupy up
// to six pages. Packets always have to be aligned on pages boundaries. Only pages 0x40 to 0x60
// are available for us to read/write on.
//
// We use pages 0x40..0x4c (12 pages) to store the pages to transmit out. While the device is
// sending it the packet at pages 0x40..0x46, we can write the packet at 0x46..0x4c, and
// vice-versa.
//
// We use pages 0x4c..0x60 (20 pages) for the device to read Ethereum packets in. When the
// device reads a packet, we need to then read it through the DMA into RAM.
//
// The circular buffer containing the pages isn't available in physical memory. In order to access
// it from the host (i.e. us), we have to use the DMA system of the chip.
//
// # Implementation note
//
// We always maintain the device in started mode and with registers page 0.
// All methods require `&mut self`, guaranteed the lack of race conditions.
// If there's a need to change the registers page, it must be set back to 0 afterwards.
//
pub struct Device {
    /// Base I/O port where to write commands to. All ports are derived from this one.
    base_port: u32,
    /// Next page to write to.
    next_write_page: u32,
    /// Page containing the next packet of incoming data to read.
    next_to_read: u8,
    /// MAC address of the device. // TODO: keep as a field? it isn't really useful
    mac_address: [u8; 6],
}

/// Range of pages that we use for the read ring buffer.
const READ_BUFFER_PAGES: Range<u8> = 0x4c..0x60;

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

        // Configuring the read ring buffer.
        redshirt_hardware_interface::port_write_u8(base_port + 1, READ_BUFFER_PAGES.start);
        redshirt_hardware_interface::port_write_u8(base_port + 3, READ_BUFFER_PAGES.start);
        redshirt_hardware_interface::port_write_u8(base_port + 2, READ_BUFFER_PAGES.end);

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

        // Writing the CURR (Current Page Register). This is the page where the device will write
        // incoming packets.
        redshirt_hardware_interface::port_write_u8(base_port + 7, READ_BUFFER_PAGES.start);
        // Registers to page 0. Abort/complete DMA and start.
        redshirt_hardware_interface::port_write_u8(base_port + 0, (1 << 5) | (1 << 1));

        // Transmit Configuration register. Normal operation.
        redshirt_hardware_interface::port_write_u8(base_port + 13, 0);
        // Receive Configuration register.
        redshirt_hardware_interface::port_write_u8(base_port + 12, (1 << 3) | (1 << 2));

        Device {
            base_port,
            next_write_page: 0x40,
            next_to_read: READ_BUFFER_PAGES.start,
            mac_address,
        }
    }

    /// Reads one packet of incoming data from the device's buffer.
    ///
    /// Returns `None` if there's no packet available.
    async unsafe fn read_one_incoming(&mut self) -> Option<Vec<u8>> {
        debug_assert!(self.next_to_read >= READ_BUFFER_PAGES.start);
        debug_assert!(self.next_to_read < READ_BUFFER_PAGES.end);

        // Read the value of the `CURR` register. It is automatically updated by the device
        // when a packet is read. We compare it with `self.next_to_read` to know whether there
        // is available data.
        if self.read_curr_register().await == self.next_to_read {
            return None;
        }

        // The device prepends each packet with a header.
        let (status, next_packet_page, current_packet_len) = {
            let mut out = [0; 4];
            dma_read(self.base_port, &mut out, self.next_to_read).await;
            let next = out[1];
            let len = u16::from_le_bytes([out[2], out[3]]);
            (out[0], out[1], len)
        };

        // TODO: check this status thing

        debug_assert!(current_packet_len < 15522);       // TODO: is that correct?
        let mut out_packet = vec![0; usize::from(current_packet_len)];
        dma_read(self.base_port, &mut out_packet, self.next_to_read);

        // Update `self.next_to_read` with the page of the next packet.
        self.next_to_read = if next_packet_page == READ_BUFFER_PAGES.end {
            READ_BUFFER_PAGES.start
        } else {
            next_packet_page
        };

        // Write in the BNRY (Boundary) register the address of the last page that we read.
        // This prevents the device from potentially overwriting packets we haven't read yet.
        if self.next_to_read == READ_BUFFER_PAGES.start {
            redshirt_hardware_interface::port_write_u8(self.base_port + 3, READ_BUFFER_PAGES.end - 1);
        } else {
            redshirt_hardware_interface::port_write_u8(self.base_port + 3, self.next_to_read - 1);
        }

        Some(out_packet)
    }

    /// Reads the value of the `CURR` register, indicating the next page the device will write a
    /// received packet to.
    async unsafe fn read_curr_register(&mut self) -> u8 {
        let mut ops = redshirt_hardware_interface::HardwareOperationsBuilder::new();

        // Registers to page 1. Abort/complete DMA and start.
        redshirt_hardware_interface::port_write_u8(self.base_port + 0, (1 << 6) | (1 << 5) | (1 << 1));

        // Read the `CURR` register.
        let mut out = 0;
        ops.port_read_u8(self.base_port + 16, &mut out);

        // Registers to page 0. Abort/complete DMA and start.
        redshirt_hardware_interface::port_write_u8(self.base_port + 0, (1 << 5) | (1 << 1));
    
        ops.send().await;
        out
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

        unimplemented!();

        ops.send();

        // TODO: wait until transmitted
    }

    /// Sends a command to the device to transmit out data from its circular buffer.
    unsafe fn send_transmit_command(&mut self, page_start: u8, len: u16) {
        let mut ops = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();

        // Set transmit page start to address where we wrote.
        ops.port_write_u8(self.base_port + 4, 0x40);
        // Length to transmit.
        let len_bytes = len.to_le_bytes();
        ops.port_write_u8(self.base_port + 5, len_bytes[0]);
        ops.port_write_u8(self.base_port + 6, len_bytes[1]);

        // Abort/complete DMA + Transmit packet + Start.
        ops.port_write_u8(self.base_port + 0, (1 << 5) | (1 << 2) | (1 << 1));

        ops.send();
    }

    pub async unsafe fn on_interrupt(&mut self) {
        // Read the ISR (Interrupt Status Register) to determine why an interrupt has been raised.
        let status = redshirt_hardware_interface::port_read_u8(self.base_port + 7).await;
        // Write back the same status in order to clear the bits and allow further interrupts to
        // happen.
        redshirt_hardware_interface::port_write_u8(self.base_port + 7, status);

        if (status & (1 << 0)) != 0 {
            // Packet received with no error.
            if let Some(packet) = read_one_incoming {
                // TODO: implement
            }
            // TODO: read packet

        } else if (status & (1 << 1)) != 0 || (status & (1 << 3)) != 0 {
            // Packet transmission successful or aborted. We don't treat the "aborted" situation
            // differently than the successful situation.
            // TODO: inform of successful packet transfer
        }
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Device").finish()
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
