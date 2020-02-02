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

use core::{cell::RefCell, convert::TryFrom as _, fmt, ops::Range, time::Duration};
use futures::{prelude::*, lock::Mutex};
use redshirt_time_interface::Delay;
use smallvec::SmallVec;

/// State of a device.
//
// # Device overview
//
// The ne2000 has a circular buffer of pages of 256 bytes each. An Ethernet packet can occupy up
// to six pages. Packets always have to be aligned on pages boundaries. Only pages 0x40 to 0x60
// are available for us to read/write on. The first 16 bytes of the device memory contain its MAC
// address.
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
// We use pages 0x40..0x4c (12 pages) to store the pages to transmit out. For example, while the
// device is sending it the packet at pages 0x40..0x46, we can write the packet at 0x46..0x4c, and
// vice-versa.
//
// We use pages 0x4c..0x60 (20 pages) for the device to read Ethereum packets in. When the
// device reads a packet, we need to then read it through the DMA into RAM.
//
// Sending out a packet is done in three steps: first we store the packet locally, waiting for
// space in the buffer to be available. Then, we copy the packet to the device's memory. Then, we
// ask the device to transfer out the packet.
//
pub struct Device {
    /// Base information about the device.
    base: DeviceBase,
    /// Information necessary for reading a packet.
    reading: Mutex<DeviceReading>,
    /// Information necessary for writing a packet.
    writing: RefCell<DeviceWriting>,
}

/// Base information about the device. Immutable. Shared between the reading and writing side.
struct DeviceBase {
    /// Base I/O port where to write commands to. All ports are derived from this one.
    base_port: u32,
    /// MAC address of the device.
    mac_address: [u8; 6],
}

/// Writing state of the device.
struct DeviceWriting {
    /// Range of pages that the device is currently transmitting out to the network.
    transmitting: Option<Range<u8>>,
    /// Page in device memory where to write our next packet.
    next_write_page: u8,
    /// Starting page and length of data in the device's memory that are waiting to be transmitted
    /// out.
    pending_transmit: SmallVec<[(u8, u16); 8]>,
    /// Packet of data waiting to be transferred to the device's memory as soon as space is
    /// available.
    // TODO: could probably be optimized here?
    pending_packet: Option<Vec<u8>>,
}

/// Reading state of the device.
struct DeviceReading {
    /// Page that contains or will contain the next incoming Ethernet packet.
    next_to_read: u8,
}

/// Range of pages that we use for the read ring buffer.
const READ_BUFFER_PAGES: Range<u8> = 0x4c..0x60;
/// Range of pages that we use for the write ring buffer.
const WRITE_BUFFER_PAGES: Range<u8> = 0x40..0x4c;

impl Device {
    /// Assumes that an ne2000 device is mapped starting at `base_port` and reinitializes it
    /// to a starting state.
    pub async unsafe fn reset(base_port: u32) -> Self {
        // Reads the RESET register and write its value back in order to reset the device.
        redshirt_hardware_interface::port_write_u8(
            base_port + 0x1f,
            redshirt_hardware_interface::port_read_u8(base_port + 0x1f).await,
        );

        // Wait for reset to be complete.
        {
            let timeout = Delay::new(Duration::from_secs(5));
            let try_reset = async move {
                loop {
                    let val = redshirt_hardware_interface::port_read_u8(base_port + 7).await;
                    if (val & 0x80) != 0 {
                        break;
                    }
                }
            };
            futures::pin_mut!(timeout);
            futures::pin_mut!(try_reset);
            match future::select(timeout, try_reset).await {
                // TODO: don't panic
                future::Either::Left(_) => panic!("timeout during reset"),
                future::Either::Right(_) => {},
            }
        }

        // Clear interrupts.
        // When an interrupt is triggered, a bit of this register is set to 1. Writing 1 resets it.
        // We reset all.
        redshirt_hardware_interface::port_write_u8(base_port + 7, 0xff);

        // Abort DMA and stop.
        redshirt_hardware_interface::port_write_u8(base_port + 0, (1 << 5) | (1 << 0));

        // Packets with multicast addresses, broadcast addresses and small are all accepted.
        redshirt_hardware_interface::port_write_u8(base_port + 12, (1 << 3) | (1 << 2) | (1 << 1));
        // External lookback. // TODO: why is that necessary?
        redshirt_hardware_interface::port_write_u8(base_port + 13, 1 << 2);

        // TODO: understand
        redshirt_hardware_interface::port_write_u8(base_port + 14, (1 << 6) | (1 << 4) | (1 << 3));

        // Read our MAC address.
        let mac_address: [u8; 6] = {
            let mut buffer = [0; 32];
            dma_read(base_port, &mut buffer, 0, 0).await;
            [
                buffer[0], buffer[2], buffer[4], buffer[6], buffer[8], buffer[10],
            ]
        };

        // TODO: remove
        redshirt_log_interface::log(
            redshirt_log_interface::Level::Info,
            &format!(
                "MAC: {:x} {:x} {:x} {:x} {:x} {:x}",
                mac_address[0],
                mac_address[1],
                mac_address[2],
                mac_address[3],
                mac_address[4],
                mac_address[5]
            ),
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
                mac_address[usize::from(n)],
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

        // TODO: why do we do this *after* starting?
        // Transmit Configuration register. Normal operation.
        redshirt_hardware_interface::port_write_u8(base_port + 13, 0);
        // Receive Configuration register.
        redshirt_hardware_interface::port_write_u8(base_port + 12, (1 << 3) | (1 << 2));

        Device {
            base: DeviceBase {
                base_port,
                mac_address,
            },
            writing: RefCell::new(DeviceWriting {
                next_write_page: WRITE_BUFFER_PAGES.start,
                pending_packet: None,
                pending_transmit: SmallVec::new(),
                transmitting: None,
            }),
            reading: Mutex::new(DeviceReading {
                next_to_read: READ_BUFFER_PAGES.start,
            }),
        }
    }

    /// Returns the MAC address of the device.
    pub fn mac_address(&self) -> [u8; 6] {
        self.base.mac_address
    }

    /// Reads one packet of incoming data from the device's buffer.
    ///
    /// Returns `None` if there's no packet available.
    pub async unsafe fn read_one_incoming(&self) -> Option<Vec<u8>> {
        let mut reading = self.reading.lock().await;

        debug_assert!(reading.next_to_read >= READ_BUFFER_PAGES.start);
        debug_assert!(reading.next_to_read < READ_BUFFER_PAGES.end);

        // Read the value of the `CURR` register. It is automatically updated by the device
        // when a packet arrives from the network.
        let curr_register = {
            let mut ops = redshirt_hardware_interface::HardwareOperationsBuilder::new();

            // Registers to page 1. Abort/complete DMA and start.
            ops.port_write_u8(self.base.base_port + 0, (1 << 6) | (1 << 5) | (1 << 1));

            // Read the register.
            let mut out = 0;
            ops.port_read_u8(self.base.base_port + 16, &mut out);

            // Registers to page 0. Abort/complete DMA and start.
            ops.port_write_u8(self.base.base_port + 0, (1 << 5) | (1 << 1));

            // Note: since the write, read, and write is sent in one chunk, it would be safe to
            // interrupt the `Future` here.
            ops.send().await;
            out
        };

        // We compare `CURR` with `reading.next_to_read` to know whether there is available data.
        //println!("curr = {:?} ; next = {:?}", curr_register, reading.next_to_read);
        if curr_register == reading.next_to_read {
            return None;
        }

        // The device prepends each packet with a header which we need to analyze.
        let (status, next_packet_page, current_packet_len) = {
            let mut out = [0; 4];
            dma_read(self.base.base_port, &mut out, reading.next_to_read, 0).await;
            let next = out[1];
            let len = u16::from_le_bytes([out[2], out[3]]);
            (out[0], out[1], len)
        };

        // TODO: why are these checks necessary?
        if status & 0x1f != 1 {
            return None;
        }
        if next_packet_page < READ_BUFFER_PAGES.start || next_packet_page > READ_BUFFER_PAGES.end {
            return None;
        }
        if current_packet_len > 1536 {
            return None;
        }

        assert!(next_packet_page >= READ_BUFFER_PAGES.start);
        assert!(next_packet_page <= READ_BUFFER_PAGES.end);

        debug_assert!(current_packet_len < 15522); // TODO: is that correct?
        let mut out_packet = vec![0; usize::from(current_packet_len)];
        dma_read(
            self.base.base_port,
            &mut out_packet,
            reading.next_to_read,
            4,
        )
        .await;

        // Update `reading.next_to_read` with the page of the next packet.
        reading.next_to_read = if next_packet_page == READ_BUFFER_PAGES.end {
            READ_BUFFER_PAGES.start
        } else {
            next_packet_page
        };

        // Write in the BNRY (Boundary) register the address of the last page that we read.
        // This prevents the device from potentially overwriting packets we haven't read yet.
        if reading.next_to_read == READ_BUFFER_PAGES.start {
            redshirt_hardware_interface::port_write_u8(
                self.base.base_port + 3,
                READ_BUFFER_PAGES.end - 1,
            );
        } else {
            redshirt_hardware_interface::port_write_u8(
                self.base.base_port + 3,
                reading.next_to_read - 1,
            );
        }

        Some(out_packet)
    }

    /// Sends a packet out. Returns an error if the device's buffer is full, in which case we must
    /// try again later.
    ///
    /// # Panic
    ///
    /// Panics if the packet is too large.
    ///
    pub unsafe fn send_packet(&self, packet: impl Into<Vec<u8>>) -> Result<(), ()> {
        let mut writing = self.writing.borrow_mut();

        if writing.pending_packet.is_some() {
            return Err(());
        }

        let packet = packet.into();
        assert!(packet.len() <= 1522);
        writing.pending_packet = Some(packet);

        flush_out(&self.base, &mut writing);

        Ok(())
    }

    // TODO:
    /*pub async unsafe fn on_interrupt(&mut self) {
        // Read the ISR (Interrupt Status Register) to determine why an interrupt has been raised.
        let status = redshirt_hardware_interface::port_read_u8(self.base_port + 7).await;
        // Write back the same status in order to clear the bits and allow further interrupts to
        // happen.
        redshirt_hardware_interface::port_write_u8(self.base_port + 7, status);

        if (status & (1 << 0)) != 0 {
            // Packet received with no error.
            if let Some(packet) = self.read_one_incoming().await {
                // TODO: implement
            }
        }

        if (status & (1 << 1)) != 0 || (status & (1 << 3)) != 0 {
            // Packet transmission successful or aborted. We don't treat the "aborted" situation
            // differently than the successful situation.
            self.transmitting = None;
            self.flush_out();
        }
    }*/
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Device").finish()
    }
}

/// Updates the state of the writing.
unsafe fn flush_out(base: &DeviceBase, writing: &mut DeviceWriting) {
    // Ask the device to transmit out more data, if some is ready.
    if writing.transmitting.is_none() && !writing.pending_transmit.is_empty() {
        let (start_page, len) = writing.pending_transmit.remove(0);
        send_transmit_command(base, writing, start_page, len);
        debug_assert!(writing.transmitting.is_some());
    }

    // Copy, if possible, `pending_packet` to the device's memory and transfer it out.
    if writing.pending_packet.is_some() {
        let pending_packet_len =
            u16::try_from(writing.pending_packet.as_ref().unwrap().len()).unwrap();
        let pending_packet_pages = u8::try_from(1 + (pending_packet_len - 1) / 256).unwrap();

        // Reset `next_write_page` to the start of the circular buffer if necessary.
        if WRITE_BUFFER_PAGES.end - writing.next_write_page < pending_packet_pages {
            writing.next_write_page = WRITE_BUFFER_PAGES.start;
        }
        debug_assert!(WRITE_BUFFER_PAGES.end - writing.next_write_page >= pending_packet_pages);

        let space_available = if let Some(transmitting) = &writing.transmitting {
            if let Some(dist_bef) = transmitting.start.checked_sub(writing.next_write_page) {
                dist_bef >= pending_packet_pages
            } else {
                debug_assert!(writing.next_write_page >= transmitting.end);
                true
            }
        } else {
            true
        };

        // Write the packet and transfer it out.
        if space_available {
            let data = writing.pending_packet.take().unwrap();
            dma_write(base.base_port, &data, writing.next_write_page);
            if writing.transmitting.is_some() {
                writing
                    .pending_transmit
                    .push((writing.next_write_page, pending_packet_len));
            } else {
                send_transmit_command(base, writing, writing.next_write_page, pending_packet_len);
            }
            writing.next_write_page += pending_packet_pages;
            debug_assert!(writing.transmitting.is_some());
        }
    }
}

/// Sends a command to the device to transmit out data from its circular buffer.
unsafe fn send_transmit_command(
    base: &DeviceBase,
    writing: &mut DeviceWriting,
    page_start: u8,
    len: u16,
) {
    debug_assert!(writing.transmitting.is_none());
    debug_assert_ne!(len, 0);

    let mut ops = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();

    // Set transmit page start to address where we wrote.
    ops.port_write_u8(base.base_port + 4, page_start);
    // Length to transmit.
    let len_bytes = len.to_le_bytes();
    ops.port_write_u8(base.base_port + 5, len_bytes[0]);
    ops.port_write_u8(base.base_port + 6, len_bytes[1]);

    // Abort/complete DMA + Transmit packet + Start.
    ops.port_write_u8(base.base_port + 0, (1 << 5) | (1 << 2) | (1 << 1));

    ops.send();

    let page_end = page_start + u8::try_from(((len - 1) / 256) + 1).unwrap();
    writing.transmitting = Some(page_start..page_end);
}

/// Reads data from the memory of the card.
///
/// Command register must be at page 0.
///
/// It is safe to cancel the `Future` while in progress. All the data will be read from the DMA
/// whatever happens.
///
/// # Safety
///
/// Race condition if the same remote memory is at the same time written by something else.
///
async unsafe fn dma_read(base_port: u32, data: &mut [u8], page_start: u8, page_offset: u8) {
    if data.is_empty() {
        return;
    }

    assert!(usize::from(page_start) + ((data.len() - 1) / 256 + 1) < 0x60);

    let (data_len_lo, data_len_hi) = if let Ok(len) = u16::try_from(data.len()) {
        let len_bytes = len.to_le_bytes();
        (len_bytes[0], len_bytes[1])
    } else {
        panic!() // TODO:
    };

    let mut ops = redshirt_hardware_interface::HardwareOperationsBuilder::new();

    // DMA remote bytes count set to the length we want to write.
    ops.port_write_u8(base_port + 10, data_len_lo);
    ops.port_write_u8(base_port + 11, data_len_hi);
    // DMA remote start address.
    ops.port_write_u8(base_port + 8, page_offset);
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
        panic!() // TODO:
    };

    let mut ops = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();

    // DMA remote bytes count set to the length we want to write.
    ops.port_write_u8(base_port + 10, data_len_lo);
    ops.port_write_u8(base_port + 11, data_len_hi);
    // DMA remote start address.
    ops.port_write_u8(base_port + 8, 0); // A page is 256 bytes, so the low is always 0
    ops.port_write_u8(base_port + 9, page_start);
    // Remote write + start.
    ops.port_write_u8(base_port + 0, (1 << 4) | (1 << 1));

    // Feed data to the DMA.
    for byte in data {
        ops.port_write_u8(base_port + 16, *byte);
    }

    ops.send();
}
