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

//! Implements the PCI interface.
//!
//! See https://en.wikipedia.org/wiki/PCI_configuration_space

// TODO: support Enhanced Configuration Access Mechanism (ECAM)

use std::{borrow::Cow, convert::TryFrom as _};

include!(concat!(env!("OUT_DIR"), "/build-pci.rs"));

fn main() {
    nametbd_syscalls_interface::block_on(async_main());
}

async fn async_main() {
    /*nametbd_interface_interface::register_interface(nametbd_pci_interface::ffi::INTERFACE)
        .await.unwrap();*/

    /*loop {
        let msg = nametbd_syscalls_interface::next_interface_message().await;
        assert_eq!(msg.interface, nametbd_stdout_interface::ffi::INTERFACE);
        let nametbd_stdout_interface::ffi::StdoutMessage::Message(message) =
            DecodeAll::decode_all(&msg.actual_data).unwrap();       // TODO: don't unwrap
        console.write(&message);
    }*/

    unsafe {
        read_pci_devices().await;
    }
}

lazy_static::lazy_static! {
    static ref PCI_DEVICES: hashbrown::HashMap<(u16, u16), (&'static str, &'static str)> = build_pci_info();
}

async unsafe fn read_pci_devices() {
    // https://wiki.osdev.org/PCI
    let pci_devices = build_pci_info();
    read_bus_pci_devices(0).await;
}

async unsafe fn read_bus_pci_devices(bus_idx: u8) {
    for device_idx in 0 .. 32 {
        for func_idx in 0 .. 8 {    // TODO: check function 0 only first
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
                    Cow::Owned(format!("Unknown <0x{:x}>", device_id))
                ),
            };

            let class_code = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0x8).await;
            let bar0 = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0x10).await;
            let bar1 = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0x14).await;
            let bar2 = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0x18).await;
            let bar3 = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0x1c).await;
            let bar4 = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0x20).await;
            let bar5 = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0x24).await;
            nametbd_stdout_interface::stdout(format!("PCI device: {} - {}; class = {:x}; header = {:x}; bar = 0x{:x}, 0x{:x}, 0x{:x}, 0x{:x}, 0x{:x}, 0x{:x}\n", vendor_name, device_name, class_code, header_ty, bar0, bar1, bar2, bar3, bar4, bar5));

            // TODO: wrong; need to enumerate other PCI buses
        }
    }
}

async unsafe fn pci_cfg_read_u32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    //assert!(bus < 256); // commented out because always true
    assert!(slot < 32);
    assert!(func < 8);
    //assert!(offset < 256) // commented out because always true
    assert_eq!(offset & 3, 0);

    let addr: u32 = 0x80000000 |
        (u32::from(bus) << 16) |
        (u32::from(slot) << 11) |
        (u32::from(func) << 8) |
        u32::from(offset);

    let mut operations_builder = nametbd_hardware_interface::HardwareOperationsBuilder::new();
    operations_builder.port_write_u32(0xcf8, addr);
    let mut out = 0;
    // TODO: is it correct to immediately read back afterwards without delay? seems weird to me
    operations_builder.port_read_u32(0xcfc, &mut out);
    operations_builder.send().await;
    out
}
