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

//! Queries the PCI interface for all the devices on the system, and prints them out.
//!
//! This program is nothing more than a small debugging utility.

use fnv::FnvBuildHasher;
use parity_scale_codec::DecodeAll;
use std::{borrow::Cow, convert::TryFrom as _};

include!(concat!(env!("OUT_DIR"), "/build-pci.rs"));

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    let devices = redshirt_pci_interface::get_pci_devices().await;

    for device in devices {
        let (vendor_name, device_name) =
            match PCI_DEVICES.get(&(device.vendor_id, device.device_id)) {
                Some((v, d)) => (Cow::Borrowed(*v), Cow::Borrowed(*d)),
                None => (
                    Cow::Owned(format!("Unknown <0x{:x}>", device.vendor_id)),
                    Cow::Owned(format!("Unknown <0x{:x}>", device.device_id)),
                ),
            };

        // TODO: print out what the class/subclass codes correspond to
        log::info!(
            "PCI device: {} - {}\nDevice class: 0x{:x} 0x{:x} 0x{:x} 0x{:x}",
            vendor_name,
            device_name,
            device.class_code,
            device.subclass,
            device.prog_if,
            device.revision_id,
        );
    }
}

lazy_static::lazy_static! {
    static ref PCI_DEVICES: hashbrown::HashMap<(u16, u16), (&'static str, &'static str), FnvBuildHasher> = build_pci_info();
}
