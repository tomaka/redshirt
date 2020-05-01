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

//! Driver for the ne2000 network card.
//!
//! This program scans the PCI space for the ne2000. If it finds it, it registers a new network
//! interface towards the network manager, and handles the communication between the network
//! manager and the hardware.
//!
//! Bibliography:
//!
//! - https://wiki.osdev.org/Ne2000
//! - https://en.wikipedia.org/wiki/NE1000#NE2000
//! - http://www.ethernut.de/pdf/8019asds.pdf
//!

use futures::prelude::*;
use redshirt_network_interface::interface;
use std::convert::TryFrom as _;

mod device;

fn main() {
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    let mut ne2k_devices = Vec::new();

    let pci_devices = redshirt_pci_interface::get_pci_devices().await;
    for device in pci_devices {
        if device.vendor_id == 0x10ec && device.device_id == 0x8029 {
            let port_number = device
                .base_address_registers
                .iter()
                .filter_map(|bar| match bar {
                    redshirt_pci_interface::PciBaseAddressRegister::Io { base_address }
                        if *base_address != 0 =>
                    {
                        Some(*base_address)
                    }
                    _ => None,
                })
                .next();

            if let Some(port_number) = port_number {
                let device = unsafe { device::Device::reset(port_number) }.await;
                let registered_device_id = redshirt_random_interface::generate_u64().await;
                let registration = interface::register_interface(interface::InterfaceConfig {
                    mac_address: device.mac_address(),
                });
                ne2k_devices.push((registration, device));
                redshirt_log_interface::emit_log(
                    redshirt_log_interface::Level::Info,
                    &format!("Initialized ne2000 at 0x{:x}", port_number),
                );
            }
        }
    }

    if ne2k_devices.is_empty() {
        return;
    }

    ne2k_devices.shrink_to_fit();

    let mut tasks = stream::FuturesUnordered::new();
    for (registration, device) in &ne2k_devices {
        tasks.push(
            async move {
                let packet = registration.packet_to_send().await;
                unsafe {
                    device.send_packet(packet).unwrap();
                } // TODO: unwrap?
            }
            .boxed_local(),
        );

        tasks.push(
            async move {
                // TODO: wake up only on interrupt
                loop {
                    if let Some(packet) = unsafe { device.read_one_incoming().await } {
                        registration.packet_from_network().await.send(packet)
                    }
                }
            }
            .boxed_local(),
        );
    }

    while let Some(_) = tasks.next().await {}
}
