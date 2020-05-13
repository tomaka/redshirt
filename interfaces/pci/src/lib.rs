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

//! Access to PCI devices.
//!
//! Use this interface if you're writing a device driver.

#![no_std]

extern crate alloc;

pub use self::ffi::{PciBaseAddressRegister, PciDeviceBdf, PciDeviceInfo};

use alloc::vec::Vec;
use futures::prelude::*;

pub mod ffi;

/// Returns the list of PCI devices available on the system.
pub fn get_pci_devices() -> impl Future<Output = Vec<PciDeviceInfo>> {
    unsafe {
        let msg = ffi::PciMessage::GetDevicesList;
        // TODO: don't unwrap?
        redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg)
            .unwrap()
            .map(|response: ffi::GetDevicesListResponse| response.devices)
    }
}

pub struct DeviceLock(ffi::PciDeviceBdf);

impl DeviceLock {
    pub async fn new(location: PciDeviceBdf) -> Result<Self, ()> {
        unsafe {
            let msg = ffi::PciMessage::LockDevice(location.clone());
            let r: Result<(), ()> =
                redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg)
                    .unwrap()
                    .await;
            r?;
            Ok(DeviceLock(location))
        }
    }

    pub fn set_command(&self, bus_master: bool, memory_space: bool, io_space: bool) {
        unsafe {
            redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &{
                ffi::PciMessage::SetCommand {
                    location: self.0.clone(),
                    bus_master,
                    memory_space,
                    io_space,
                }
            })
            .unwrap();
        }
    }
}

impl Drop for DeviceLock {
    fn drop(&mut self) {
        unsafe {
            redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &{
                ffi::PciMessage::UnlockDevice(self.0.clone())
            })
            .unwrap();
        }
    }
}
