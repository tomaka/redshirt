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
    0x2d, 0x93, 0xd0, 0x48, 0xab, 0x88, 0x1c, 0x95, 0x87, 0xf5, 0x5c, 0x3b, 0xe6, 0x4d, 0x8f, 0x65,
    0x3f, 0x37, 0x4c, 0x4e, 0xad, 0xea, 0x15, 0xcc, 0xf0, 0x17, 0x44, 0x0f, 0x6d, 0x6e, 0x5d, 0xc8,
]);

/// Message in destination to the PCI interface handler.
#[derive(Debug, Encode, Decode)]
pub enum PciMessage {
    /// Request list of PCI devices. Answered with a [`GetDevicesListResponse`].
    GetDevicesList,

    /// Makes the current process as the "owner" of the given PCI device.
    ///
    /// Returns a SCALE-encoded `Ok(())` if the locking worked, and a SCALE-encoded `Err(())` if
    /// the device has already been locked.
    // TODO: no, proper answer
    LockDevice(PciDeviceBdf),

    /// Unlocks a previously-locked device.
    ///
    /// Has no effect if the device wasn't locked by the current process.
    ///
    /// Doesn't return any answer.
    ///
    /// Answers all the pending [`PciMessage::NextInterrupt`] messages for this device.
    UnlockDevice(PciDeviceBdf),

    /// Produces a [`NextInterruptResponse`] answer when the next interrupt from the PCI device
    /// happens. The PCI must have been locked.
    ///
    /// Note that multiple PCI devices might share the same interrupt line, and spurious answers
    /// might therefore happen. The `status` register of the PCI device is not necessarily checked.
    ///
    /// For clean-up reasons, answers are also triggered if you unlock the device.
    NextInterrupt(PciDeviceBdf),

    /// Read or write the configuration space of a device.
    // TODO: forbid writing some parts such as the BARs
    ConfigurationSpaceOperations {
        /// Device to access. Must have been previously locked.
        device: PciDeviceBdf,
        operations: Vec<MemoryOperation>,
    },

    /// Read or write the mapped memory of a PCI device.
    ///
    /// Answers with a SCALE-encoded `Vec<MemoryAccessResponse>` containins one element per
    /// successful read.
    BarMemoryOperations {
        /// Device to access. Must have been previously locked.
        device: PciDeviceBdf,
        /// Which BAR (Base Address Register) is concerned. Must in the range `0..6`.
        bar_offset: u8,
        /// List of operations to perform.
        operations: Vec<MemoryOperation>,
    },

    /// Read or write the I/O ports accessing a PCI device.
    ///
    /// Answers with a SCALE-encoded `Vec<IoAccessResponse>` containins one element per
    /// successful read.
    BarIoOperations {
        /// Device to access. Must have been previously locked.
        device: PciDeviceBdf,
        /// Which BAR (Base Address Register) is concerned. Must in the range `0..6`.
        bar_offset: u8,
        /// List of operations to perform.
        operations: Vec<IoOperation>,
    },
}

/// Response to [`PciMessage::GetDevicesList`].
#[derive(Debug, Encode, Decode)]
pub struct GetDevicesListResponse {
    /// List of PCI devices available on the system.
    pub devices: Vec<PciDeviceInfo>,
}

/// Response to [`PciMessage::NextInterrupt`].
#[derive(Debug, Encode, Decode)]
pub enum NextInterruptResponse {
    /// Success. We got an interrupt.
    Interrupt,

    /// Returned if the specified device isn't locked.
    BadDevice,

    /// Returned if the specified device was locked but got unlocked before an interrupt happened.
    Unlocked,
}

/// Location of a PCI device according to the controller.
///
/// > **Note**: The acronym BDF stands for "Bus, Device, Function".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Encode, Decode)]
pub struct PciDeviceBdf {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

/// Description of a single PCI device.
#[derive(Debug, Clone, Encode, Decode)]
pub struct PciDeviceInfo {
    /// Location of the device on the machine. Uniquely identifies each device.
    pub location: PciDeviceBdf,

    pub vendor_id: u16,
    pub device_id: u16,
    pub base_address_registers: Vec<PciBaseAddressRegister>,
    // TODO: add more fields
}

/// Description of a single PCI device.
// TODO: actually figure out PCI and adjust this
#[derive(Debug, Clone, Encode, Decode)]
pub enum PciBaseAddressRegister {
    Memory { base_address: u32 },
    Io { base_address: u32 },
}

/// Request to perform accesses to memory-mapped memory.
#[derive(Debug, Encode, Decode)]
pub enum MemoryOperation {
    Memset {
        offset: u64,
        len: u64,
        value: u8,
    },
    WriteU8 {
        offset: u64,
        data: Vec<u8>,
    },
    /// Uses the platform's native endianess.
    WriteU16 {
        offset: u64,
        data: Vec<u16>,
    },
    /// Uses the platform's native endianess.
    WriteU32 {
        offset: u64,
        data: Vec<u32>,
    },
    ReadU8 {
        offset: u64,
        len: u32,
    },
    ReadU16 {
        offset: u64,
        /// Number of `u16`s to read.
        len: u32,
    },
    ReadU32 {
        offset: u64,
        /// Number of `u32`s to read.
        len: u32,
    },
}

/// Request to perform accesses to I/O ports.
#[derive(Debug, Encode, Decode)]
pub enum IoOperation {
    /// Write data to a port.
    WriteU8 {
        /// Offset of the port from the value in the BAR.
        port_offset: u32,
        /// Data to write.
        data: u8,
    },
    /// Write data to a port.
    WriteU16 {
        /// Offset of the port from the value in the BAR.
        port_offset: u32,
        /// Data to write.
        data: u16,
    },
    /// Write data to a port.
    WriteU32 {
        /// Offset of the port from the value in the BAR.
        port_offset: u32,
        /// Data to write.
        data: u32,
    },
    /// Reads data from a port.
    ReadU8 {
        /// Offset of the port from the value in the BAR.
        port_offset: u32,
    },
    /// Reads data from a port.
    ReadU16 {
        /// Offset of the port from the value in the BAR.
        port_offset: u32,
    },
    /// Reads data from a port.
    ReadU32 {
        /// Offset of the port from the value in the BAR.
        port_offset: u32,
    },
}

/// Response to a [`PciMessage::BarMemoryOperations`].
#[derive(Debug, Encode, Decode)]
pub enum MemoryAccessResponse {
    /// Sent back in response to a [`MemoryOperation::ReadU8`].
    ReadU8(Vec<u8>),
    /// Sent back in response to a [`MemoryOperation::ReadU16`].
    ReadU16(Vec<u16>),
    /// Sent back in response to a [`MemoryOperation::ReadU32`].
    ReadU32(Vec<u32>),
}

/// Response to a [`PciMessage::BarIoOperations`].
#[derive(Debug, Encode, Decode)]
pub enum IoAccessResponse {
    /// Sent back in response to a [`IoOperation::ReadU8`].
    ReadU8(u8),
    /// Sent back in response to a [`IoOperation::ReadU16`].
    ReadU16(u16),
    /// Sent back in response to a [`IoOperation::ReadU32`].
    ReadU32(u32),
}
