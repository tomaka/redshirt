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
    /// Request list of PCI devices. Answer with a [`GetDevicesListResponse`].
    GetDevicesList,
}

/// Response to [`PciMessage::GetDevicesList`].
#[derive(Debug, Encode, Decode)]
pub struct GetDevicesListResponse {
    /// List of PCI devices available on the system.
    pub devices: Vec<PciDeviceInfo>,
}

/// Description of a single PCI device.
#[derive(Debug, Clone, Encode, Decode)]
pub struct PciDeviceInfo {
    pub vendor_id: u16,
    pub device_id: u16,
    pub base_address_registers: Vec<PciBaseAddressRegister>,
    // TODO: add more fields
}

/// Description of a single PCI device.
// TODO: actually figure out PCI and adjust this
#[derive(Debug, Clone, Encode, Decode)]
pub enum PciBaseAddressRegister {
    Memory {
        base_address: u32,
        prefetchable: bool,
    },
    Io {
        base_address: u32,
    },
}
