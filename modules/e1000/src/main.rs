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

//! Driver for the Intel 8254x network cards family.
//!
//! This family of network cards is widely implemented in emulators such as QEmu, BOCHS, or
//! VirtualBox.
//!
//! This program scans the PCI space for the e1000. If it finds it, it registers a new network
//! interface towards the network manager, and handles the communication between the network
//! manager and the hardware.

use futures::prelude::*;
use redshirt_ethernet_interface::interface;

mod device;

fn main() {
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    let mut e1000_devices = Vec::new();

    let pci_devices = redshirt_pci_interface::get_pci_devices().await;
    for device in pci_devices {
        // List of all the devices that we support.
        // While there exists a wide range of features that some devices support and some others
        // don't, we only support the common denominator of features between all these devices.
        match (device.vendor_id, device.device_id) {
            (0x8086, 0x100e)
            | (0x8086, 0x100f)
            | (0x8086, 0x1010)
            | (0x8086, 0x1011)
            | (0x8086, 0x1012)
            | (0x8086, 0x1013)
            | (0x8086, 0x1015)
            | (0x8086, 0x1016)
            | (0x8086, 0x1017)
            | (0x8086, 0x1018)
            | (0x8086, 0x1019)
            | (0x8086, 0x101a)
            | (0x8086, 0x101d)
            | (0x8086, 0x1026)
            | (0x8086, 0x1027)
            | (0x8086, 0x1028)
            | (0x8086, 0x1076)
            | (0x8086, 0x1077)
            | (0x8086, 0x1078)
            | (0x8086, 0x1079)
            | (0x8086, 0x107a)
            | (0x8086, 0x107b)
            | (0x8086, 0x1107)
            | (0x8086, 0x1112) => {}
            _ => continue,
        };

        let base_address = device
            .base_address_registers
            .iter()
            .filter_map(|bar| match bar {
                redshirt_pci_interface::PciBaseAddressRegister::Memory { base_address }
                    if *base_address != 0 =>
                {
                    Some(*base_address)
                }
                _ => None,
            })
            .next();

        let base_address = match base_address {
            Some(p) => p,
            None => continue,
        };

        let pci_lock = match redshirt_pci_interface::PciDeviceLock::lock(device.location).await {
            Ok(l) => l,
            // PCI device is already handled by a different driver.
            Err(_) => continue,
        };

        // Start by resetting the device, in case it was active.
        let device_prototype = match unsafe { device::Device::reset(base_address.into()) }.await {
            Ok(p) => p,
            Err(_) => continue,
        };

        // We need to inform the PCI bus that this device needs access to RAM.
        // We only do this *after* resetting the device, otherwise it might have accidentally been
        // still active and trying to perform memory writes.
        pci_lock.set_command(true, true, false);

        // Now that the device has access to memory, finish initialization.
        let device = match device_prototype.init().await {
            Ok(d) => d,
            Err(_) => continue,
        };

        // Inform the Ethernet interface that a device is available.
        let registration = interface::register_interface(interface::InterfaceConfig {
            mac_address: device.mac_address(),
        })
        .await;

        // `e1000_devices` is processed after we have finished enumerating all PCI devices.
        e1000_devices.push((registration, pci_lock, device));
    }

    // Now that devices detection has been performed, we switch to phase 2: processing the
    // detected devices.

    if e1000_devices.is_empty() {
        return;
    }
    e1000_devices.shrink_to_fit();

    let mut tasks = stream::FuturesUnordered::new();
    for (registration, pci_lock, device) in &e1000_devices {
        // This task is dedicated to sending out packets in destination to the device.
        tasks.push(
            async move {
                loop {
                    let packet = registration.packet_to_send().await;
                    unsafe {
                        // TODO: this can panic if we are trying to send out data at a faster
                        // rate than the device is capable of
                        device.send_packet(packet).await.unwrap();
                    }
                }
            }
            .boxed_local(),
        );

        // This task is dedicated to listening to IRQs and pulling packets received from the
        // network.
        tasks.push(
            async move {
                loop {
                    // Note that we grab a future to the next IRQ *before* calling `on_interrupt`,
                    // otherwise IRQs that happen right after `on_interrupt` has returned wouldn't
                    // be caught.
                    let next_interrupt = pci_lock.next_interrupt();

                    for packet in unsafe { device.on_interrupt().await } {
                        registration.packet_from_network().await.send(packet)
                    }

                    next_interrupt.await;
                }
            }
            .boxed_local(),
        );
    }

    // Processing the tasks here.
    while let Some(_) = tasks.next().await {}
}
