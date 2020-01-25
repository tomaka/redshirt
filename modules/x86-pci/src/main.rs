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

//! Implements the PCI interface.
//!
//! See https://en.wikipedia.org/wiki/PCI_configuration_space

// TODO: support Enhanced Configuration Access Mechanism (ECAM)

use fnv::FnvBuildHasher;
use parity_scale_codec::DecodeAll;
use std::{borrow::Cow, convert::TryFrom as _};

include!(concat!(env!("OUT_DIR"), "/build-pci.rs"));

fn main() {
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    redshirt_interface_interface::register_interface(redshirt_pci_interface::ffi::INTERFACE)
        .await
        .unwrap();

    let devices = unsafe { read_pci_devices().await };

    loop {
        let msg = match redshirt_syscalls::next_interface_message().await {
            redshirt_syscalls::DecodedInterfaceOrDestroyed::Interface(m) => m,
            redshirt_syscalls::DecodedInterfaceOrDestroyed::ProcessDestroyed(_) => continue,
        };
        assert_eq!(msg.interface, redshirt_pci_interface::ffi::INTERFACE);
        let redshirt_pci_interface::ffi::PciMessage::GetDevicesList =
            DecodeAll::decode_all(&msg.actual_data.0).unwrap(); // TODO: don't unwrap; also, crappy decoding
        redshirt_syscalls::emit_answer(
            msg.message_id.unwrap(),
            &redshirt_pci_interface::ffi::GetDevicesListResponse {
                devices: devices.clone(),
            },
        );
    }
}

lazy_static::lazy_static! {
    static ref PCI_DEVICES: hashbrown::HashMap<(u16, u16), (&'static str, &'static str), FnvBuildHasher> = build_pci_info();
}

async unsafe fn read_pci_devices() -> Vec<redshirt_pci_interface::PciDeviceInfo> {
    // https://wiki.osdev.org/PCI
    let pci_devices = build_pci_info();
    read_bus_pci_devices(0).await
}

async unsafe fn read_bus_pci_devices(bus_idx: u8) -> Vec<redshirt_pci_interface::PciDeviceInfo> {
    let mut out = Vec::new();

    for device_idx in 0..32 {
        for func_idx in 0..8 {
            // TODO: check function 0 only first
            let (vendor_id, device_id) = {
                let vendor_device = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0).await;
                let vendor_id = u16::try_from(vendor_device & 0xffff).unwrap();
                let device_id = u16::try_from(vendor_device >> 16).unwrap();
                (vendor_id, device_id)
            };

            if vendor_id == 0xffff {
                continue;
            }

            let (_bist, header_ty, latency, cache_line) = {
                let val = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0xc).await;
                let bytes = val.to_be_bytes();
                (bytes[0], bytes[1], bytes[2], bytes[3])
            };

            let (vendor_name, device_name) = match PCI_DEVICES.get(&(vendor_id, device_id)) {
                Some((v, d)) => (Cow::Borrowed(*v), Cow::Borrowed(*d)),
                None => (
                    Cow::Owned(format!("Unknown <0x{:x}>", vendor_id)),
                    Cow::Owned(format!("Unknown <0x{:x}>", device_id)),
                ),
            };

            let class_code = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0x8).await;

            out.push(redshirt_pci_interface::PciDeviceInfo {
                vendor_id,
                device_id,
                base_address_registers: {
                    let mut list = Vec::with_capacity(6);
                    for bar_n in 0..6 {
                        let bar =
                            pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0x10 + bar_n * 0x4)
                                .await;
                        list.push(if (bar & 0x1) == 0 {
                            let prefetchable = (bar & (1 << 3)) != 0;
                            let base_address = bar & !0b1111;
                            redshirt_pci_interface::PciBaseAddressRegister::Memory {
                                base_address,
                                prefetchable,
                            }
                        } else {
                            let base_address = bar & !0b11;
                            redshirt_pci_interface::PciBaseAddressRegister::Io { base_address }
                        });
                    }
                    list
                },
            });

            redshirt_log_interface::log(
                redshirt_log_interface::Level::Info,
                &format!("PCI device: {} - {}", vendor_name, device_name),
            );

            // TODO: wrong; need to enumerate other PCI buses
        }
    }

    out
}

// TODO: ensure endianess? PCI is always little endian, but what if we're on a BE platform?
async unsafe fn pci_cfg_read_u32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    //assert!(bus < 256); // commented out because always true
    assert!(slot < 32);
    assert!(func < 8);
    //assert!(offset < 256) // commented out because always true
    assert_eq!(offset & 3, 0);

    let addr: u32 = 0x80000000
        | (u32::from(bus) << 16)
        | (u32::from(slot) << 11)
        | (u32::from(func) << 8)
        | u32::from(offset);

    let mut operations_builder = redshirt_hardware_interface::HardwareOperationsBuilder::new();
    operations_builder.port_write_u32(0xcf8, addr);
    let mut out = 0;
    // TODO: is it correct to immediately read back afterwards without delay? seems weird to me
    operations_builder.port_read_u32(0xcfc, &mut out);
    operations_builder.send().await;
    out
}
