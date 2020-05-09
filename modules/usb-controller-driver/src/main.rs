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

//! UHCI, OHCI, EHCI and xHCI driver.
//!
//! This program detects PCI devices that correspond to USB host controllers, and implements the
//! USB interface.
// TODO: only OHCI is implemented lol

use futures::prelude::*;
use parity_scale_codec::DecodeAll;
use std::{borrow::Cow, convert::TryFrom as _};

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    for device in redshirt_pci_interface::get_pci_devices().await {
        match (device.class_code, device.subclass, device.prog_if) {
            (0xc, 0x3, 0x0) => {
                // UHCI
                unimplemented!() // TODO:
            }
            (0xc, 0x3, 0x10) => {
                // OHCI
                let addr = match device.base_address_registers[0] {
                    redshirt_pci_interface::PciBaseAddressRegister::Memory { base_address } => {
                        u64::from(base_address)
                    }
                    _ => unreachable!(), // TODO: don't panic
                };

                log::info!("Initializing OHCI device at 0x{:x}", addr);
                unsafe {
                    usb_controller_driver::ohci::init_ohci_device(HwAccess, addr)
                        .await
                        .unwrap();
                }
            }
            (0xc, 0x3, 0x20) => {
                // EHCI
                unimplemented!() // TODO:
            }
            (0xc, 0x3, 0x30) => {
                // xHCI
                unimplemented!() // TODO:
            }
            _ => {}
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct HwAccess;
unsafe impl<'a> usb_controller_driver::HwAccessRef<'a> for &'a HwAccess {
    type ReadMemFutureU8 = future::BoxFuture<'a, ()>;
    type ReadMemFutureU32 = future::BoxFuture<'a, ()>;
    type WriteMemFutureU8 = future::Ready<()>;
    type WriteMemFutureU32 = future::Ready<()>;

    unsafe fn read_memory_u8(self, address: u64, dest: &'a mut [u8]) -> Self::ReadMemFutureU8 {
        let mut builder = redshirt_hardware_interface::HardwareOperationsBuilder::new();
        builder.read(address, dest);
        builder.send().boxed()
    }

    unsafe fn read_memory_u32(self, address: u64, dest: &'a mut [u32]) -> Self::ReadMemFutureU32 {
        let mut builder = redshirt_hardware_interface::HardwareOperationsBuilder::new();
        builder.read_u32(address, dest);
        builder.send().boxed()
    }

    unsafe fn write_memory_u8(self, address: u64, data: &[u8]) -> Self::WriteMemFutureU8 {
        redshirt_hardware_interface::write(address, data.to_vec());
        future::ready(())
    }

    unsafe fn write_memory_u32(self, address: u64, data: &[u32]) -> Self::WriteMemFutureU32 {
        let mut builder = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();
        // TODO: optimize
        for (off, elem) in data.iter().enumerate() {
            builder.write_one_u32(address + (off as u64) * 4, *elem);
        }
        builder.send();
        future::ready(())
    }
}
