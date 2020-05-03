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

//! Manages the PCI devices of the system.
//!
//! See https://en.wikipedia.org/wiki/PCI_configuration_space

use core::{borrow::Cow, convert::TryFrom as _};
use fnv::FnvBuildHasher;
use x86_64::structures::port::{PortRead as _, PortWrite as _};

/// Initializes PCI the "legacy" way, by reading and writing CPU I/O ports.
///
/// # Safety
///
/// The PCI space must only be enabled once.
// TODO: support Enhanced Configuration Access Mechanism (ECAM)
pub unsafe fn init_cam_pci() -> PciDevices {
    PciDevices {
        known_devices: Vec::new(),      // FIXME:
    }
}

/// Manages PCI devices.
pub struct PciDevices {
    /// Result of the devices scan.
    /// Never modified.
    known_devices: Vec<DeviceInfo>,
}

struct DeviceBdf {
    bus: u8,
    device: u8,
    function: u8,
}

struct DeviceInfo {
    bdf: DeviceBdf,
    vendor_id: u16,
    device_id: u16,
}

impl PciDevices {
    // TODO:
    pub fn devices(&self) -> impl Iterator<Item = Device> {
        self.known_devices.iter().map(|_| ())
    }
}

/// Access to a single device within the list.
pub struct Device<'a> {
    parent: &'a PciDevices,
    bdf: DeviceBdf,
}

impl<'a> Device<'a> {
    pub fn bus(&self) -> u8 {
        self.bdf.bus
    }

    pub fn device(&self) -> u8 {
        self.bdf.device
    }

    pub fn function(&self) -> u8 {
        self.bdf.function
    }

    pub fn base_address_registers(&self) -> impl Iterator<Item = BaseAddressRegister> {

    }
}

#[derive(Debug)]
pub enum BaseAddressRegister {
    Memory {
        base_address: usize,
        prefetchable: bool,
    },
    Io {
        base_address: u16,
    },
}

unsafe fn read_bus_pci_devices(bus_idx: u8) -> Vec<DeviceInfo> {
    let mut out = Vec::new();

    for device_idx in 0..32 {
        for func_idx in 0..8 {
            // TODO: check function 0 only first
            let bdf = DeviceBdf {
                bus: bus_idx,
                device: device_idx,
                function: func_idx,
            };

            read_pci_slot(&bdf);
        }
    }
}

/// Reads the information about a single PCI slot. Returns the list of devices that have been found.
///
/// # Panic
///
/// Panics if the device is out of range.
fn scan_device(bus: u8, device: u8) -> Vec<DeviceInfo> {
    assert!(device < 32);

    let function0 = scan_function(bus, device, 0);
}

/// Reads the information about a single PCI slot. Returns the list of devices that have been found.
unsafe fn scan_function(bdf: &DeviceBdf) -> Vec<DeviceInfo> {
    let (vendor_id, device_id) = {
        let vendor_device = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0);
        let vendor_id = u16::try_from(vendor_device & 0xffff).unwrap();
        let device_id = u16::try_from(vendor_device >> 16).unwrap();
        (vendor_id, device_id)
    };

    if vendor_id == 0xffff {
        return None;
    }

    let (class_code, subclass, _prog_if, _revision_id) = {
        let val = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0x8);
        let bytes = val.to_be_bytes();
        (bytes[0], bytes[1], bytes[2], bytes[3])
    };

    let (_bist, header_ty, latency, cache_line) = {
        let val = pci_cfg_read_u32(bus_idx, device_idx, func_idx, 0xc);
        let bytes = val.to_be_bytes();
        (bytes[0], bytes[1], bytes[2], bytes[3])
    };

    out.push(DeviceInfo {
        bus: bus_idx,
        device: device_idx,
        function: func_idx,
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
}

/// Reads the configuration space of the given device.
///
/// Automatically swaps bytes on big-endian platforms.
///
/// # Panic
///
/// Panics if the device or function are out of range.
/// Panics if `offset` is not 4-bytes aligned.
///
unsafe fn pci_cfg_read_u32(bdf: &DeviceBdf, offset: u8) -> u32 {
    assert!(bdf.device < 32);
    assert!(bdf.function < 8);
    assert_eq!(offset % 4, 0);

    let addr: u32 = 0x80000000
        | (u32::from(bdf.bus) << 16)
        | (u32::from(bdf.device) << 11)
        | (u32::from(bdf.function) << 8)
        | u32::from(offset);

    u32::write_to_port(0xcf8, addr);
    if cfg!(target_endian = "little") {
        u32::read_from_port(0xcfc)
    } else {
        u32::read_from_port(0xcfc).swap_bytes()
    }
}
