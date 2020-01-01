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

//! Access to PCI devices.
//!
//! Use this interface if you're writing a device driver.

#![deny(intra_doc_link_resolution_failure)]
#![no_std]

extern crate alloc;

pub use self::ffi::{PciBaseAddressRegister, PciDeviceInfo};

use alloc::vec::Vec;
use futures::prelude::*;

pub mod ffi;

/// Returns the list of PCI devices available on the system.
pub fn get_pci_devices() -> impl Future<Output = Vec<PciDeviceInfo>> {
    unsafe {
        let msg = ffi::PciMessage::GetDevicesList;
        // TODO: don't unwrap?
        redshirt_syscalls_interface::emit_message_with_response(&ffi::INTERFACE, msg)
            .unwrap()
            .map(|response: ffi::GetDevicesListResponse| response.devices)
    }
}
