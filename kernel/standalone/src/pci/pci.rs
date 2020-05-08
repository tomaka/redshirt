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

use alloc::{borrow::Cow, collections::VecDeque, vec::Vec};
use core::{convert::TryFrom as _, iter};
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
        known_devices: scan_all_buses(),
    }
}

/// Manages PCI devices.
pub struct PciDevices {
    /// Result of the devices scan.
    /// Never modified.
    known_devices: Vec<DeviceInfo>,
}

#[derive(Debug, Copy, Clone)]
struct DeviceBdf {
    bus: u8,
    device: u8,
    function: u8,
}

#[derive(Debug)]
struct DeviceInfo {
    bdf: DeviceBdf,
    vendor_id: u16,
    device_id: u16,
    header_ty: u8,
    class_code: u8,
    subclass: u8,
    prog_if: u8,
    revision_id: u8,
    base_address_registers: Vec<BaseAddressRegister>,
}

impl PciDevices {
    // TODO:
    pub fn devices(&self) -> impl Iterator<Item = Device> {
        (0..self.known_devices.len()).map(move |index| Device {
            parent: self,
            index,
        })
    }
}

/// Access to a single device within the list.
pub struct Device<'a> {
    parent: &'a PciDevices,
    index: usize,
}

impl<'a> Device<'a> {
    pub fn bus(&self) -> u8 {
        self.parent.known_devices[self.index].bdf.bus
    }

    pub fn device(&self) -> u8 {
        self.parent.known_devices[self.index].bdf.device
    }

    pub fn function(&self) -> u8 {
        self.parent.known_devices[self.index].bdf.function
    }

    pub fn vendor_id(&self) -> u16 {
        self.parent.known_devices[self.index].vendor_id
    }

    pub fn device_id(&self) -> u16 {
        self.parent.known_devices[self.index].device_id
    }

    pub fn class_code(&self) -> u8 {
        self.parent.known_devices[self.index].class_code
    }

    pub fn subclass(&self) -> u8 {
        self.parent.known_devices[self.index].subclass
    }

    pub fn prog_if(&self) -> u8 {
        self.parent.known_devices[self.index].prog_if
    }

    pub fn revision_id(&self) -> u8 {
        self.parent.known_devices[self.index].revision_id
    }

    pub fn base_address_registers(&self) -> impl Iterator<Item = BaseAddressRegister> + 'a {
        self.parent.known_devices[self.index]
            .base_address_registers
            .iter()
            .cloned()
    }
}

#[derive(Debug, Copy, Clone)]
pub enum BaseAddressRegister {
    Memory {
        base_address: usize,
        prefetchable: bool,
    },
    Io {
        base_address: u16,
    },
}

/// Scans all the PCI devices.
fn scan_all_buses() -> Vec<DeviceInfo> {
    // TODO: apparently it's possible to have multiple PCI controllers
    //       see https://wiki.osdev.org/PCI#Recursive_Scan

    let mut checked = Vec::with_capacity(32);
    let mut to_check = VecDeque::with_capacity(32);
    to_check.push_back(0);
    let mut out = Vec::with_capacity(64);

    loop {
        let next_bus = match to_check.pop_front() {
            Some(b) => b,
            None => return out,
        };

        debug_assert!(!checked.iter().any(|b| *b == next_bus));
        checked.push(next_bus);

        for scan_result in scan_bus(next_bus) {
            match scan_result {
                ScanResult::Device(dev) => out.push(dev),
                ScanResult::Bridge { target_bus, .. } => {
                    if !checked.iter().any(|b| *b == next_bus) {
                        to_check.push_back(target_bus);
                    }
                }
            }
        }
    }
}

/// Scans all the devices on a certain PCI bus.
fn scan_bus(bus: u8) -> impl Iterator<Item = ScanResult> {
    (0..32).flat_map(move |device_idx| scan_device(bus, device_idx))
}

/// Reads the information about a single PCI device. Returns the list of devices that have been found.
///
/// # Panic
///
/// Panics if the device is out of range.
fn scan_device(bus: u8, device: u8) -> impl Iterator<Item = ScanResult> {
    assert!(device < 32);

    let f0 = match scan_function(&DeviceBdf {
        bus,
        device,
        function: 0,
    }) {
        Some(f) => f,
        None => return either::Right(iter::empty()),
    };

    match f0 {
        // If the MSB of `header_ty` is 1, then this is a "multi-function" device and we need to
        // scan all the other functions.
        ScanResult::Device(info) if (info.header_ty & 0x80) != 0 => {
            let iter = (1..=7).flat_map(move |func_idx| {
                scan_function(&DeviceBdf {
                    bus,
                    device,
                    function: func_idx,
                })
                .into_iter()
            });

            either::Left(either::Right(iter))
        }
        f0 => either::Left(either::Left(iter::once(f0))),
    }
}

/// Output of [`scan_function`].
#[derive(Debug)]
enum ScanResult {
    /// Function is a device description.
    Device(DeviceInfo),
    /// Function is a bridge to a different bus.
    Bridge {
        /// Location of the function that we have scanned.
        bdf: DeviceBdf,
        /// Bus in question.
        target_bus: u8,
    },
}

/// Reads the information about a single PCI slot, or `None` if it is empty.
///
/// # Panic
///
/// Panics if the device is out of range.
fn scan_function(bdf: &DeviceBdf) -> Option<ScanResult> {
    let (vendor_id, device_id) = {
        let vendor_device = pci_cfg_read_u32(bdf, 0);
        let vendor_id = u16::try_from(vendor_device & 0xffff).unwrap();
        let device_id = u16::try_from(vendor_device >> 16).unwrap();
        (vendor_id, device_id)
    };

    if vendor_id == 0xffff {
        return None;
    }

    let (class_code, subclass, prog_if, revision_id) = {
        let val = pci_cfg_read_u32(bdf, 0x8);
        let bytes = val.to_be_bytes();
        (bytes[0], bytes[1], bytes[2], bytes[3])
    };

    let (_bist, header_ty, latency, cache_line) = {
        let val = pci_cfg_read_u32(bdf, 0xc);
        let bytes = val.to_be_bytes();
        (bytes[0], bytes[1], bytes[2], bytes[3])
    };

    // This class/subclass combination indicates a PCI-to-PCI bridge, for which we return
    // something different.
    if class_code == 0x7 && subclass == 0x4 {
        let (_sec_latency_timer, _sub_bus_num, sec_bus_num, _prim_bus_num) = {
            let val = pci_cfg_read_u32(bdf, 0x18);
            let bytes = val.to_be_bytes();
            (bytes[0], bytes[1], bytes[2], bytes[3])
        };

        return Some(ScanResult::Bridge {
            bdf: *bdf,
            target_bus: sec_bus_num,
        });
    }

    Some(ScanResult::Device(DeviceInfo {
        bdf: *bdf,
        vendor_id,
        device_id,
        header_ty,
        class_code,
        subclass,
        prog_if,
        revision_id,
        base_address_registers: {
            let mut list = Vec::with_capacity(6);
            for bar_n in 0..6 {
                let bar = pci_cfg_read_u32(bdf, 0x10 + bar_n * 0x4);
                list.push(if (bar & 0x1) == 0 {
                    let prefetchable = (bar & (1 << 3)) != 0;
                    let base_address = usize::try_from(bar & !0b1111).unwrap();
                    BaseAddressRegister::Memory {
                        base_address,
                        prefetchable,
                    }
                } else {
                    let base_address = u16::try_from(bar & !0b11).unwrap();
                    BaseAddressRegister::Io { base_address }
                });
            }
            list
        },
    }))
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
fn pci_cfg_read_u32(bdf: &DeviceBdf, offset: u8) -> u32 {
    assert!(bdf.device < 32);
    assert!(bdf.function < 8);
    assert_eq!(offset % 4, 0);

    let addr: u32 = 0x80000000
        | (u32::from(bdf.bus) << 16)
        | (u32::from(bdf.device) << 11)
        | (u32::from(bdf.function) << 8)
        | u32::from(offset);

    unsafe {
        u32::write_to_port(0xcf8, addr);
        if cfg!(target_endian = "little") {
            u32::read_from_port(0xcfc)
        } else {
            u32::read_from_port(0xcfc).swap_bytes()
        }
    }
}
