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

//! Bibliography:
//!
//! - https://www.intel.com/content/dam/doc/manual/pci-pci-x-family-gbe-controllers-software-dev-manual.pdf
//! - https://wiki.osdev.org/Intel_8254x
//!

use core::{convert::TryFrom as _, fmt, mem, time::Duration};
use futures::lock::Mutex;
use redshirt_hardware_interface::malloc::PhysicalBuffer;
use redshirt_time_interface::Delay;

/// State of a device.
//
// # Device overview
//
// To use a e1000-compatible card, one must allocate two ring buffers in physical memory, one
// receive descriptors ring buffer and one transmit descriptors ring buffer, and configure to
// device to use these ring buffers.
//
// These two ring buffers contain *descriptors* indicating the read or write operation to perform.
// These descriptors contain pointers to other buffers that contain the actual data.
//
// While the device is active, it will automatically process the content of these ring buffers.
// This driver only needs to write a single register (the read descripor tail or transmit
// descriptor tail) to indicate to the device that the ring buffer has been updated. Once the
// device has written incoming data in a read descriptor or transmitted data in a transmit
// descriptor, it writes a flag in the descriptor itself, which this driver can read to determine
// that the descriptor either contains data or is available for a new transmit.
pub struct Device {
    /// Base physical address of the memory-mapped registers.
    regs_base_address: u64,
    /// MAC address of the device.
    mac_address: [u8; 6],

    /// Ring buffer of receive descriptors. Read by the hardware.
    receive_descriptors: PhysicalBuffer<[ReceiveDescriptor]>,
    /// For each descriptor in the ring buffer, a buffer where the data is going to be received.
    receive_buffers: Vec<PhysicalBuffer<[u8]>>,
    /// Index within `receive_buffers` of the next descriptor that is expected to receive data.
    /// The `Mutex` must be locked while the descriptor in question is being checked. If the
    /// receive descriptor tail register must be written, it must be done before unlocking this
    /// mutex to guarantee the lack of race conditions.
    receive_next: Mutex<usize>,

    /// Ring buffer of transmit descriptors. Read by the hardware.
    transmit_descriptors: PhysicalBuffer<[TransmitDescriptor]>,
    /// For each descriptor in the ring buffer, a buffer from where the data is located.
    /// While we could allocate a new buffer for every single write, re-using the same buffers
    /// every time also saves us from having to properly track lifetimes.
    transmit_buffers: Vec<PhysicalBuffer<[u8]>>,
    /// State of the transmit descriptors. If the transmit descriptor tail register must be
    /// written, it must be done before unlocking this mutex to guarantee the lack of race
    /// conditions.
    transmit_state: Mutex<TransmitState>,
}

struct TransmitState {
    /// Index in the transmit descriptors of the next transmit descriptor that we can use to send
    /// something. Always corresponds to the "transmit queue tail" register.
    next_available: usize,

    /// Index in the transmit descriptors of the next transmit descriptor that the hardware is
    /// currently sending and that we will attempt to reclaim.
    /// If `next_reclaim` is equal to `next_available`, then there's an ambiguity as to whether
    /// the queue is full or empty. To avoid this, we never fully reclaim all the transmit
    /// descriptors and always leave at least one transmit descriptor owned by the hardware.
    /// Therefore, if the two values are equal it means that the queue is full.
    next_reclaim: usize,
}

// List of registers, which their offset, in bytes, relative to the base mapping address.
const REGS_CTRL: u64 = 0x0;
const REGS_STATUS: u64 = 0x8;
const REGS_FCAL: u64 = 0x28;
const REGS_FCAH: u64 = 0x2c;
const REGS_FCT: u64 = 0x30;
const REGS_ICR: u64 = 0xc0;
const REGS_ITR: u64 = 0xc4;
const REGS_IMS: u64 = 0xd0;
const REGS_IMC: u64 = 0xd8;
const REGS_RCTL: u64 = 0x100;
const REGS_FCTTV: u64 = 0x170;
const REGS_TCTL: u64 = 0x400;
const REGS_RDBAL: u64 = 0x2800;
const REGS_RDBAH: u64 = 0x2804;
const REGS_RDLEN: u64 = 0x2808;
const REGS_RDH: u64 = 0x2810;
const REGS_RDT: u64 = 0x2818;
const REGS_RDTR: u64 = 0x2820;
const REGS_TDBAL: u64 = 0x3800;
const REGS_TDBAH: u64 = 0x3804;
const REGS_TDLEN: u64 = 0x3808;
const REGS_TDH: u64 = 0x3810;
const REGS_TDT: u64 = 0x3818;
const REGS_MTA_BASE: u64 = 0x5200;
const REGS_RAL: u64 = 0x5400;
const REGS_RAH: u64 = 0x5404;

#[derive(Debug, Copy, Clone)]
#[repr(packed)]
struct ReceiveDescriptor {
    buffer_address: u64,
    length: u16,
    // reserved, on some devices
    checksum: u16,
    status: u8,
    errors: u8,
    // reserved, on some devices
    special: u16,
}

// This is a so-called "legacy" descriptor.
#[derive(Debug, Copy, Clone)]
#[repr(packed)]
struct TransmitDescriptor {
    buffer_address: u64,
    length: u16,
    checksum_offset: u8,
    command: u8,
    status: u8, // also includes a "reserved" field
    checksum_start: u8,
    special: u16,
}

impl Device {
    /// Assumes that a e1000 device is mapped starting at `base_port` and reinitializes it
    /// to a starting state.
    pub async unsafe fn reset(regs_base_address: u64) -> Result<DevicePrototype, InitErr> {
        // Set the RST flag in order to reset the device.
        redshirt_hardware_interface::write_one_u32(
            regs_base_address + REGS_CTRL,
            redshirt_hardware_interface::read_one_u32(regs_base_address + REGS_CTRL).await
                | (1 << 26),
        );

        // Specs recommend to wait for 1µs.
        Delay::new(Duration::from_micros(1)).await;

        // Wait for reset to be complete.
        {
            let mut attempts = 0;
            loop {
                attempts += 1;
                if attempts >= 1000 {
                    return Err(InitErr::Timeout);
                }

                let val =
                    redshirt_hardware_interface::read_one_u32(regs_base_address + REGS_CTRL).await;
                if (val & (1 << 26)) == 0 {
                    break;
                }

                Delay::new(Duration::from_millis(5)).await;
            }
        }

        Ok(DevicePrototype { regs_base_address })
    }

    /// Returns the MAC address of the device.
    pub fn mac_address(&self) -> [u8; 6] {
        self.mac_address
    }

    /// Reads the next pending Ethernet frame waiting to be delivered, if any.
    ///
    /// Returns `None` if there's no packet available.
    ///
    /// > **Note**: This function is asynchronous, but it returns as soon as it determines that
    /// >           no packet is available. It does *not* wait for one packet to be available.
    pub async unsafe fn read_one_incoming(&self) -> Option<Vec<u8>> {
        let mut receive_next = self.receive_next.lock().await;
        debug_assert!(*receive_next < self.receive_descriptors.len());

        // Read the next descriptor that we expect the hardware to write to.
        let mut next_descriptor: ReceiveDescriptor = self
            .receive_descriptors
            .read_one(*receive_next)
            .await
            .unwrap();

        // When the hardware has written the descriptor, it sets this bit on the status to mark
        // it as ready. If the bit is not set, no packet is ready.
        if next_descriptor.status & (1 << 0) == 0 {
            return None;
        }

        // Normally, there is a N-to-one mapping between descriptors and packets. In other words,
        // packets can occupy more than one descriptor. However, since we use 16 kiB buffers, it
        // is guaranteed that packets will never use more than one descriptor.
        // Check the "end of packet" bit.
        assert!(next_descriptor.status & (1 << 1) != 0);

        // The `errors` field indicates whether the packet is a correct or a faulty packet.
        // We configure the device to not put faulty packets in the receive queue, and thus
        // shouldn't find any.
        assert_eq!(next_descriptor.errors, 0);

        // Now reading the actual packet of data.
        let packet = {
            let mut packet = Vec::<u8>::with_capacity(usize::from(next_descriptor.length));
            packet.set_len(packet.capacity());
            self.receive_buffers[*receive_next]
                .read_slice(0, &mut packet[..])
                .await;
            packet
        };

        // Overwrite the status byte of the descriptor to 0 to set it as "non-ready". This is not
        // a requirement from the hardware, but it is necessary for us to differentiate ready from
        // non-ready descriptors.
        // TODO: optimize by writing only the status byte; needs `PhysicalBuffer` API tweaking
        next_descriptor.status = 0;
        self.receive_descriptors
            .write_one(*receive_next, next_descriptor);

        // Inform the hardware of the new tail. Note that the tail is one-past the last descriptor
        // that he hardware is allowed to write. Therefore this write doesn't permit the hardware
        // to write on `receive_next`, but it permits the hardware to write on `receive_next - 1`.
        redshirt_hardware_interface::write_one_u32(
            self.regs_base_address + REGS_RDT,
            u32::try_from(*receive_next).unwrap(),
        );

        // Update `receive_next` for the next time.
        *receive_next = (*receive_next + 1) % self.receive_descriptors.len();

        // Success!
        Some(packet)
    }

    /// Sends a packet out. Returns an error if the device's buffer is full, in which case we must
    /// try again later.
    ///
    /// # Panic
    ///
    /// Panics if the packet is too large.
    ///
    pub async unsafe fn send_packet<T>(&self, packet: T) -> Result<(), T>
    where
        T: AsRef<[u8]>,
    {
        let mut transmit_state = self.transmit_state.lock().await;
        debug_assert!(transmit_state.next_available < self.transmit_descriptors.len());
        debug_assert!(transmit_state.next_reclaim < self.transmit_descriptors.len());

        // If our local head and tail are equal, it means that the queue is full.
        if transmit_state.next_available == transmit_state.next_reclaim {
            // We try to solve the queue being full by reclaiming descriptors whose transmission
            // is finished.
            loop {
                // Don't reclaim too much!
                let potential_next_reclaim =
                    (transmit_state.next_reclaim + 1) % self.transmit_descriptors.len();
                if potential_next_reclaim == transmit_state.next_available {
                    break;
                }

                // Read the next descriptor that we expect the hardware to have finished
                // processing.
                let next_descriptor: TransmitDescriptor = self
                    .transmit_descriptors
                    .read_one(transmit_state.next_reclaim)
                    .await
                    .unwrap();

                // When the hardware has finished processing the descriptor, it sets this bit on
                // the status to mark it as done. If the bit is not set, the packet is still
                // being transferred.
                if next_descriptor.status & (1 << 0) == 0 {
                    break;
                }

                // We can now treat this descriptor as reclaimed by incrementing `next_reclaim`.
                transmit_state.next_reclaim = potential_next_reclaim;
            }
        }

        // If it is still full, return with an error.
        if transmit_state.next_available == transmit_state.next_reclaim {
            return Err(packet);
        }

        // If we reach here, we know that `next_available` is free.

        // Write the actual packet of data.
        self.transmit_buffers[transmit_state.next_available].write_slice(0, packet.as_ref());

        // Write the transmit descriptor itself.
        self.transmit_descriptors.write_one(
            transmit_state.next_available,
            TransmitDescriptor {
                buffer_address: self.transmit_buffers[transmit_state.next_available].pointer(),
                length: u16::try_from(packet.as_ref().len()).unwrap(),
                checksum_offset: 0,
                command: {
                    let mut cmd = 0;
                    cmd |= 1 << 0; // This descriptor is the end of the packet.
                    cmd |= 1 << 1; // Insert the CRC field in the packet.
                    cmd |= 1 << 3; // Hardware should write back the "status" field when done.
                    cmd
                },
                status: 0,
                checksum_start: 0,
                special: 0,
            },
        );

        // Bump the value for the next time.
        transmit_state.next_available =
            (transmit_state.next_available + 1) % self.transmit_descriptors.len();

        // Inform the hardware of the new tail. The hardware will start transmitting this
        // descriptor.
        redshirt_hardware_interface::write_one_u32(
            self.regs_base_address + REGS_TDT,
            u32::try_from(transmit_state.next_available).unwrap(),
        );

        Ok(())
    }

    /// Must be called when the device generates an interrupt.
    ///
    /// Returns a packet of data received from the network, if any.
    pub async unsafe fn on_interrupt(&self) -> Vec<Vec<u8>> {
        // Note: there exists a register indicating the cause for the interrupt, but we choose to
        // ignore it and check everything.
        // However, the action of reading it has the side-effect of clearing its bits, which is
        // necessary for the next interrupt to be triggered.
        let _ = redshirt_hardware_interface::read_one_u32(self.regs_base_address + REGS_ICR).await;

        let mut out = Vec::with_capacity(8);
        while let Some(packet) = self.read_one_incoming().await {
            out.push(packet);
        }
        out
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Device")
            .field("regs_base_address", &self.regs_base_address)
            .field("mac_address", &self.mac_address)
            .finish()
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            // Set the RST flag in order to reset the device.
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_CTRL, 1 << 26);
        }
    }
}

/// Error that can happen during [`Device::reset`] or [`DevicePrototype::init`].
#[derive(Debug, derive_more::Display)]
pub enum InitErr {
    /// Device is taking too long to respond.
    Timeout,
}

/// A device after it has been reinitialized but before it is active.
pub struct DevicePrototype {
    /// Base physical address of the mapped memory of this device.
    regs_base_address: u64,
}

impl DevicePrototype {
    /// Finish initializing the device.
    pub async fn init(self) -> Result<Device, InitErr> {
        unsafe {
            // Documentation mentions that we should write 0 to the flow control registers if
            // we don't use flow control.
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_FCAL, 0);
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_FCAH, 0);
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_FCT, 0);
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_FCTTV, 0);

            // We set the flags of the CTRL register, effectively enabling the link with the
            // outside.
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_CTRL, {
                let mut ctl =
                    redshirt_hardware_interface::read_one_u32(self.regs_base_address + REGS_CTRL)
                        .await;
                ctl &= !(1 << 3); // Stop resetting link
                ctl |= 1 << 5; // Automatic speed detection
                ctl |= 1 << 6; // Set link up
                ctl &= !(1 << 7); // Documentation says this bit should be cleared
                ctl &= !(1 << 30); // Documentation says this bit should be cleared
                ctl &= !(1 << 31); // Documentation says this bit should be cleared
                ctl
            });

            // The interrupt throttling register prevents interrupts from being generated too
            // often.
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_ITR, {
                // The value to pass is multiplied by 256ns to obtain the minimum interval between
                // two interrupts.
                // Here we set this interval to the arbitrary value of 1ms.
                1000 * 4
            });

            // The documentation mentions that one must clear the IMS and IMC in order to prevent
            // a deadlock using the 82547GI/EI.
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_IMS, 0xffff);
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_IMC, 0xffff);

            // The IMS decides which interrupts are generated by the device.
            // It is a "set when written" kind of register. In other words, writing 0 has no effect
            // and writing 1 sets the bit.
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_IMS, {
                let mut mask = 0;
                // TODO: should we process link broken interrupts? is there something to do? look
                // in specs
                mask |= 1 << 4; // Went over threshold for occupied space in receive descriptors.
                mask |= 1 << 6; // Receive descriptors buffer full.
                mask |= 1 << 7; // Timer after data has been received.
                mask
            });

            // Delay between a received packet and the interrupt. Recommended to be set to 0, as
            // we benefit from `REGS_ITR` instead.
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_RDTR, 0);

            // Reading the MAC address.
            let mac_address: [u8; 6] = {
                let lo =
                    redshirt_hardware_interface::read_one_u32(self.regs_base_address + REGS_RAL)
                        .await
                        .to_le_bytes();
                let hi =
                    redshirt_hardware_interface::read_one_u32(self.regs_base_address + REGS_RAH)
                        .await
                        .to_le_bytes();
                [lo[0], lo[1], lo[2], lo[3], hi[0], hi[1]]
            };

            // Configure the receive descriptor ring buffer.
            let (receive_descriptors, receive_buffers) = {
                let receive_descriptors = PhysicalBuffer::new_uninit_slice_with_align(32, 16)
                    .await
                    .assume_init();

                // Each receive descriptor has an associated 16 kiB buffer where the packet will be
                // written to.
                let receive_buffers = {
                    let mut b = Vec::with_capacity(receive_descriptors.len());
                    for _ in 0..receive_descriptors.len() {
                        b.push(
                            PhysicalBuffer::new_uninit_slice(16 * 1024)
                                .await
                                .assume_init(),
                        );
                    }
                    b
                };

                // Filling `receive_descriptors`. Only the `buffer_address` field is read by the
                // hardware, while all the other fields will be written.
                for n in 0..receive_descriptors.len() {
                    receive_descriptors.write_one(
                        n,
                        ReceiveDescriptor {
                            buffer_address: receive_buffers[n].pointer(),
                            length: 0,
                            checksum: 0,
                            status: 0,
                            errors: 0,
                            special: 0,
                        },
                    );
                }

                // Indicate to the device the ring buffer address.
                let address = receive_descriptors.pointer();
                assert_eq!(address % 16, 0);
                redshirt_hardware_interface::write_one_u32(
                    self.regs_base_address + REGS_RDBAH,
                    u32::try_from(address >> 32).unwrap(),
                );
                redshirt_hardware_interface::write_one_u32(
                    self.regs_base_address + REGS_RDBAL,
                    u32::try_from(address & 0xffffffff).unwrap(),
                );

                // Length of the ring buffer.
                // "16" is the size in bytes of each descriptor.
                debug_assert_eq!(mem::size_of::<ReceiveDescriptor>(), 16);
                let rdlen = u32::try_from(receive_descriptors.len() * 16).unwrap();
                assert_eq!(rdlen % 128, 0);
                redshirt_hardware_interface::write_one_u32(
                    self.regs_base_address + REGS_RDLEN,
                    rdlen,
                );
                // Head of the ring buffer.
                redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_RDH, 0);
                // Tail of the ring buffer. The tail is defined as the last descriptor that is
                // part of the buffer (not a one-past-the-end value).
                redshirt_hardware_interface::write_one_u32(
                    self.regs_base_address + REGS_RDT,
                    u32::try_from(receive_descriptors.len() - 1).unwrap(),
                );

                // The MTA registers are filters for incoming packets. Initializing everything to
                // 0 as recommended by documentation, even though these filters will normally not
                // be used.
                for n in 0..128 {
                    redshirt_hardware_interface::write_one_u32(
                        self.regs_base_address + REGS_MTA_BASE + n * 4,
                        0,
                    );
                }

                // Read control register.
                redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_RCTL, {
                    let mut rctl = 0u32;
                    rctl |= 1 << 1; // Enable reading
                    rctl |= 1 << 3; // Don't filter unicast packets
                    rctl |= 1 << 4; // Don't filter multicast packets
                    rctl |= 1 << 5; // Receive long packets
                    rctl |= 0b00 << 8; // Interrupt generated when free space is <1/2 of total
                    rctl |= 1 << 15; // Accept broadcast packets
                    rctl |= 0b01 << 16; // Buffer size = 1 kiB, but...
                    rctl |= 1 << 25; // Multiply buffer size by 16, so it's actually 16 kiB
                    rctl |= 1 << 26; // Strip Ethernet checksum field
                    rctl
                });

                (receive_descriptors, receive_buffers)
            };

            // Configure the transmit descriptor ring buffer.
            let transmit_descriptors = {
                // We fill the transmit descriptors by making it look like the hardware has
                // previously successfully performed the write.
                let transmit_descriptors = PhysicalBuffer::new_uninit_slice_with_align(32, 16)
                    .await
                    .assume_init();
                for n in 0..transmit_descriptors.len() {
                    transmit_descriptors.write_one(
                        n,
                        TransmitDescriptor {
                            buffer_address: 0,
                            length: 0,
                            checksum_offset: 0,
                            command: 0,
                            status: 1,
                            checksum_start: 0,
                            special: 0,
                        },
                    )
                }

                // Indicate to the device the ring buffer address.
                let address = transmit_descriptors.pointer();
                assert_eq!(address % 16, 0);
                redshirt_hardware_interface::write_one_u32(
                    self.regs_base_address + REGS_TDBAH,
                    u32::try_from(address >> 32).unwrap(),
                );
                redshirt_hardware_interface::write_one_u32(
                    self.regs_base_address + REGS_TDBAL,
                    u32::try_from(address & 0xffffffff).unwrap(),
                );

                // Length of the ring buffer.
                // "16" is the size in bytes of each descriptor.
                debug_assert_eq!(mem::size_of::<TransmitDescriptor>(), 16);
                let tdlen = u32::try_from(receive_descriptors.len() * 16).unwrap();
                assert_eq!(tdlen % 128, 0);
                redshirt_hardware_interface::write_one_u32(
                    self.regs_base_address + REGS_TDLEN,
                    tdlen,
                );
                // Head of the ring buffer.
                redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_TDH, 0);
                // Tail of the ring buffer. The tail is defined as one beyond the last descriptor
                // that the hardware might read from. By setting it to 0, the hardware won't read
                // from any descriptor.
                redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_TDT, 0);

                // Transmit control register.
                redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_TCTL, {
                    let mut tctl = 0u32;
                    tctl |= 1 << 1; // Enable writing
                    tctl |= 1 << 3; // Automatically pad packets that are too short
                                    // Number of attempts before we give up transmitting. 0xf is
                                    // the recommended value.
                    tctl |= 0xf << 4;
                    // Number of garbage bytes to use for collision detection.
                    // Recommended: 0x40 for full-duplex, 0x200 for half-duplex.
                    tctl |= 0x40 << 12;
                    tctl
                });

                transmit_descriptors
            };

            // Each transmit descriptor has an associated 16 kiB buffer where the actual packet is
            // located.
            let transmit_buffers = {
                let mut b = Vec::with_capacity(transmit_descriptors.len());
                for _ in 0..transmit_descriptors.len() {
                    b.push(
                        PhysicalBuffer::new_uninit_slice(16 * 1024)
                            .await
                            .assume_init(),
                    );
                }
                b
            };

            // Wait for link to be up.
            {
                let mut attempts = 0u32;
                loop {
                    attempts += 1;
                    if attempts >= 1000 {
                        return Err(InitErr::Timeout);
                    }

                    let val = redshirt_hardware_interface::read_one_u32(
                        self.regs_base_address + REGS_STATUS,
                    )
                    .await;
                    if (val & (1 << 1)) != 0 {
                        break;
                    }

                    Delay::new(Duration::from_millis(5)).await;
                }
            }

            // Since the `Drop` implementation of the prototype resets the device again (by
            // safety), we finish the initialization by mem::forget-ting the prototype.
            let next_reclaim = transmit_buffers.len() - 1;
            let device = Device {
                regs_base_address: self.regs_base_address,
                mac_address,
                receive_descriptors,
                receive_buffers,
                receive_next: Mutex::new(0),
                transmit_descriptors,
                transmit_buffers,
                transmit_state: Mutex::new(TransmitState {
                    next_available: 0,
                    next_reclaim,
                }),
            };
            mem::forget(self);
            Ok(device)
        }
    }
}

impl fmt::Debug for DevicePrototype {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("DevicePrototype")
            .field("regs_base_address", &self.regs_base_address)
            .finish()
    }
}

impl Drop for DevicePrototype {
    fn drop(&mut self) {
        unsafe {
            // Set the RST flag in order to reset the device.
            redshirt_hardware_interface::write_one_u32(self.regs_base_address + REGS_CTRL, 1 << 26);
        }
    }
}
