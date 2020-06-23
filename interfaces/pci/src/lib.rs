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

pub use self::ffi::{PciBaseAddressRegister, PciDeviceInfo};

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

// TODO: provide a good API for all this

/// Active lock of a PCI device.
///
/// While this struct is alive, no other program can lock that same PCI device.
pub struct PciDeviceLock {
    device: ffi::PciDeviceBdf,
}

impl PciDeviceLock {
    // TODO: shouldn't be public?
    pub async fn lock(bdf: ffi::PciDeviceBdf) -> Result<Self, ()> {
        let result: Result<(), ()> = unsafe {
            let msg = ffi::PciMessage::LockDevice(bdf.clone());
            redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg).unwrap().await
        };

        result?;

        Ok(PciDeviceLock {
            device: bdf,
        })
    }

    /// Waits until the device produces an interrupt.
    ///
    /// The returned future is disconnected from the [`PciDeviceLock`]. However, polling the
    /// future after its corresponding [`PciDeviceLock`] has been destroyed will panic.
    pub fn next_interrupt(&self) -> impl Future<Output = ()> + Send + 'static {
        let bdf = self.device.clone();

        async move {
            let msg = ffi::PciMessage::NextInterrupt(bdf);
            unsafe { redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg) }
                .unwrap()
                .map(|response: ffi::NextInterruptResponse| {
                    match response {
                        ffi::NextInterruptResponse::Interrupt => {},
                        ffi::NextInterruptResponse::BadDevice => panic!(),
                        ffi::NextInterruptResponse::Unlocked => unreachable!(),
                    }
                })
                .await
        }
    }
}

impl Drop for PciDeviceLock {
    fn drop(&mut self) {
        unsafe {
            let msg = ffi::PciMessage::UnlockDevice(self.device.clone());
            redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, msg).unwrap();
        }
    }
}
