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

use alloc::vec::Vec;
use parity_scale_codec::{Decode, Encode};
use redshirt_syscalls::InterfaceHash;

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0x24, 0x5d, 0x25, 0x5e, 0x37, 0xf1, 0x8a, 0xce, 0x23, 0xd6, 0x68, 0xe9, 0xe2, 0xd8, 0xd1, 0xbc,
    0x37, 0xf3, 0xd3, 0x3c, 0xad, 0x55, 0xf8, 0xd9, 0x22, 0x3a, 0x57, 0xd1, 0x54, 0x46, 0x7b, 0x78,
]);

/// Message in destination to the hardware interface handler.
#[derive(Debug, Encode, Decode)]
pub enum HardwareMessage {
    /// Allocate RAM. Must answer with a `u64`. The value `0` is returned if the allocation is
    /// too large.
    ///
    /// This is useful in situations where you want to pass a pointer to a device.
    Malloc {
        /// Size to allocate.
        size: u64,
        /// Alignment of the pointer to return.
        ///
        /// The returned value modulo `alignment` must be equal to 0.
        alignment: u8,
    },
    /// Opposite of malloc.
    Free {
        /// Value previously returned after a malloc message.
        ptr: u64,
    },
    /// Request to perform some access on the physical memory or ports.
    ///
    /// All operations must be performed in order.
    ///
    /// If there is at least one memory or port read, the response must be a
    /// `Vec<HardwareAccessResponse>` where each element corresponds to a read. No response is
    /// expected if there are only writes.
    // TODO: should we enforce some limits in the amount of data that can be returned in a response?
    HardwareAccess(Vec<Operation>),

    /// Ask the handler to send back a response when the interrupt with the given number is
    /// triggered.
    ///
    /// > **Note**: If called with a non-hardware interrupt, no response will ever come back.
    // TODO: how to not miss any interrupt? we instead need some registration system or something
    InterruptWait(u32),
}

/// Request to perform accesses to physical memory or to ports.
#[derive(Debug, Encode, Decode)]
pub enum Operation {
    PhysicalMemoryMemset {
        address: u64,
        len: u64,
        value: u8,
    },
    PhysicalMemoryWriteU8 {
        address: u64,
        data: Vec<u8>,
    },
    /// Uses the platform's native endianess.
    PhysicalMemoryWriteU16 {
        address: u64,
        data: Vec<u16>,
    },
    /// Uses the platform's native endianess.
    PhysicalMemoryWriteU32 {
        address: u64,
        data: Vec<u32>,
    },
    PhysicalMemoryReadU8 {
        address: u64,
        len: u32,
    },
    PhysicalMemoryReadU16 {
        address: u64,
        /// Number of `u16`s to read.
        len: u32,
    },
    PhysicalMemoryReadU32 {
        address: u64,
        /// Number of `u32`s to read.
        len: u32,
    },
    /// Write data to a port.
    ///
    /// If the hardware doesn't support this operation, then nothing happens.
    PortWriteU8 {
        port: u32,
        data: u8,
    },
    /// Write data to a port.
    ///
    /// If the hardware doesn't support this operation, then nothing happens.
    PortWriteU16 {
        port: u32,
        data: u16,
    },
    /// Write data to a port.
    ///
    /// If the hardware doesn't support this operation, then nothing happens.
    PortWriteU32 {
        port: u32,
        data: u32,
    },
    /// Reads data from a port.
    ///
    /// If the hardware doesn't support this operation, then `0` is produced.
    PortReadU8 {
        port: u32,
    },
    /// Reads data from a port.
    ///
    /// If the hardware doesn't support this operation, then `0` is produced.
    PortReadU16 {
        port: u32,
    },
    /// Reads data from a port.
    ///
    /// If the hardware doesn't support this operation, then `0` is produced.
    PortReadU32 {
        port: u32,
    },
}

/// Response to a [`HardwareMessage::HardwareAccess`].
#[derive(Debug, Encode, Decode)]
pub enum HardwareAccessResponse {
    /// Sent back in response to a [`Operation::PhysicalMemoryReadU8`].
    PhysicalMemoryReadU8(Vec<u8>),
    /// Sent back in response to a [`Operation::PhysicalMemoryReadU16`].
    PhysicalMemoryReadU16(Vec<u16>),
    /// Sent back in response to a [`Operation::PhysicalMemoryReadU32`].
    PhysicalMemoryReadU32(Vec<u32>),
    /// Sent back in response to a [`Operation::PortReadU8`].
    PortReadU8(u8),
    /// Sent back in response to a [`Operation::PortReadU16`].
    PortReadU16(u16),
    /// Sent back in response to a [`Operation::PortReadU32`].
    PortReadU32(u32),
}
