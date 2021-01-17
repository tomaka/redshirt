// Copyright (C) 2019-2021  Pierre Krieger
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

use alloc::boxed::Box;
use core::{convert::TryFrom as _, mem, ptr::NonNull};
use x86_64::structures::port::{PortRead as _, PortWrite as _};

/// Loads ACPI tables from physical memory.
///
/// # Panic
///
/// Panics if the multiboot header doesn't contain any information about the ACPI tables, or if
/// the ACPI tables are invalid.
///
pub fn load_acpi_tables(
    multiboot_info: &multiboot2::BootInformation,
) -> acpi::AcpiTables<DummyAcpiHandler> {
    unsafe {
        let mut err = None;

        if let Some(rsdp_v2) = multiboot_info.rsdp_v2_tag() {
            match acpi::AcpiTables::from_rsdt(
                DummyAcpiHandler,
                rsdp_v2.revision(),
                rsdp_v2.xsdt_address(),
            ) {
                Ok(acpi) => return acpi,
                Err(e) => err = Some(e),
            }
        }

        if let Some(rsdp_v1) = multiboot_info.rsdp_v1_tag() {
            match acpi::AcpiTables::from_rsdt(
                DummyAcpiHandler,
                rsdp_v1.revision(),
                rsdp_v1.rsdt_address(),
            ) {
                Ok(acpi) => return acpi,
                Err(e) => {
                    if err.is_none() {
                        err = Some(e);
                    }
                }
            }
        }

        if let Some(err) = err {
            panic!("Couldn't parse ACPI tables: {:?}", err)
        } else {
            panic!("Can't find ACPI tables")
        }
    }
}

/// Loads and parses ACPI tables from physical memory.
///
/// # Panic
///
/// Panics if the multiboot header doesn't contain any information about the ACPI tables, or if
/// the ACPI tables are invalid.
///
pub fn parse_acpi_tables(
    multiboot_info: &multiboot2::BootInformation,
) -> acpi::AcpiTables<DummyAcpiHandler> {
    let acpi_tables = load_acpi_tables(multiboot_info);
    let mut aml = aml::AmlContext::new(Box::new(DummyAmlHandler), false, aml::DebugVerbosity::None);

    if let Some(dsdt) = &acpi_tables.dsdt {
        let stream = unsafe {
            core::slice::from_raw_parts(
                dsdt.address as *const u8,
                usize::try_from(dsdt.length).unwrap(),
            )
        };

        // TODO: AML tables parsing currently fails on VirtualBox
        let _ = aml.parse_table(stream);
    }

    acpi_tables
}

/// Implementation of the `AcpiHandler` trait that is responsible for mapping physical memory
/// into virtual memory.
///
/// We use identity mapping over the whole address space, therefore this is a dummy.
#[derive(Debug, Clone)]
pub struct DummyAcpiHandler;

impl acpi::AcpiHandler for DummyAcpiHandler {
    unsafe fn map_physical_region<T>(
        &self,
        addr: usize,
        size: usize,
    ) -> acpi::PhysicalMapping<DummyAcpiHandler, T> {
        acpi::PhysicalMapping {
            physical_start: addr,
            virtual_start: NonNull::new(addr as *mut _).unwrap(),
            region_length: size,
            mapped_length: size,
            handler: self.clone(),
        }
    }

    fn unmap_physical_region<T>(&self, _: &acpi::PhysicalMapping<DummyAcpiHandler, T>) {}
}

/// Implementation of the `Handler` trait of `aml`.
///
/// We use identity mapping over the whole address space, therefore this is a dummy.
struct DummyAmlHandler;

impl aml::Handler for DummyAmlHandler {
    fn read_u8(&self, address: usize) -> u8 {
        assert_eq!(address % mem::align_of::<u8>(), 0);
        unsafe { (address as *const u8).read() }
    }

    fn read_u16(&self, address: usize) -> u16 {
        assert_eq!(address % mem::align_of::<u16>(), 0);
        unsafe { (address as *const u16).read() }
    }

    fn read_u32(&self, address: usize) -> u32 {
        assert_eq!(address % mem::align_of::<u32>(), 0);
        unsafe { (address as *const u32).read() }
    }

    fn read_u64(&self, address: usize) -> u64 {
        assert_eq!(address % mem::align_of::<u64>(), 0);
        unsafe { (address as *const u64).read() }
    }

    fn write_u8(&mut self, address: usize, value: u8) {
        assert_eq!(address % mem::align_of::<u8>(), 0);
        unsafe { (address as *mut u8).write(value) }
    }

    fn write_u16(&mut self, address: usize, value: u16) {
        assert_eq!(address % mem::align_of::<u16>(), 0);
        unsafe { (address as *mut u16).write(value) }
    }

    fn write_u32(&mut self, address: usize, value: u32) {
        assert_eq!(address % mem::align_of::<u32>(), 0);
        unsafe { (address as *mut u32).write(value) }
    }

    fn write_u64(&mut self, address: usize, value: u64) {
        assert_eq!(address % mem::align_of::<u64>(), 0);
        unsafe { (address as *mut u64).write(value) }
    }

    fn read_io_u8(&self, port: u16) -> u8 {
        unsafe { u8::read_from_port(port) }
    }

    fn read_io_u16(&self, port: u16) -> u16 {
        unsafe { u16::read_from_port(port) }
    }

    fn read_io_u32(&self, port: u16) -> u32 {
        unsafe { u32::read_from_port(port) }
    }

    fn write_io_u8(&self, port: u16, value: u8) {
        unsafe {
            u8::write_to_port(port, value);
        }
    }

    fn write_io_u16(&self, port: u16, value: u16) {
        unsafe {
            u16::write_to_port(port, value);
        }
    }

    fn write_io_u32(&self, port: u16, value: u32) {
        unsafe {
            u32::write_to_port(port, value);
        }
    }

    fn read_pci_u8(&self, _: u16, _: u8, _: u8, _: u8, _: u16) -> u8 {
        todo!()
    }

    fn read_pci_u16(&self, _: u16, _: u8, _: u8, _: u8, _: u16) -> u16 {
        todo!()
    }

    fn read_pci_u32(&self, _: u16, _: u8, _: u8, _: u8, _: u16) -> u32 {
        todo!()
    }

    fn write_pci_u8(&self, _: u16, _: u8, _: u8, _: u8, _: u16, _: u8) {
        todo!()
    }

    fn write_pci_u16(&self, _: u16, _: u8, _: u8, _: u8, _: u16, _: u16) {
        todo!()
    }

    fn write_pci_u32(&self, _: u16, _: u8, _: u8, _: u8, _: u16, _: u32) {
        todo!()
    }
}
