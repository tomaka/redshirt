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

mod device;

use std::convert::TryFrom as _;

fn main() {
    redshirt_syscalls_interface::block_on(async_main());
}

async fn async_main() {
    let mut ne2k_devices = Vec::new();

    let pci_devices = redshirt_pci_interface::get_pci_devices().await;
    for device in pci_devices {
        if device.vendor_id == 0x10ec && device.device_id == 0x8029 {
            let port_number = device.base_address_registers.iter().filter_map(|bar| {
                match bar {
                    redshirt_pci_interface::PciBaseAddressRegister::Io { base_address } if *base_address != 0 => Some(*base_address),
                    _ => None
                }
            }).next();

            if let Some(port_number) = port_number {
                unsafe {
                    ne2k_devices.push(device::Device::reset(port_number).await);
                    redshirt_log_interface::log(
                        redshirt_log_interface::Level::Info,
                        format!("Initialized ne2000 at 0x{:x}", port_number)
                    );
                }
            }
        }
    }

    if ne2k_devices.is_empty() {
        return;
    }

    ne2k_devices.shrink_to_fit();

    loop {
        //redshirt_log_interface::log(redshirt_log_interface::Level::Info, format!("Polling"));
        let packet = match unsafe { ne2k_devices[0].read_one_incoming().await } {
            Some(p) => p,
            None => continue,
        };

        let (header, data) = etherparse::Ethernet2Header::read_from_slice(&packet).unwrap();
        if header.ether_type == 0x86dd {
            let (ip_header, ip_data) = etherparse::Ipv6Header::read_from_slice(&data).unwrap();
            if ip_header.next_header == 0x11 {
                let (udp_header, udp_data) = etherparse::UdpHeader::read_from_slice(&ip_data).unwrap();
                redshirt_log_interface::log(
                    redshirt_log_interface::Level::Info,
                    format!("Headers: {:?} {:?}", ip_header, udp_header)
                );
            }
        }

        //redshirt_log_interface::log(redshirt_log_interface::Level::Info, format!("Header: {:?}", header));
    }
}
