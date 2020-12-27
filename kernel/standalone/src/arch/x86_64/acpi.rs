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

use core::ptr::NonNull;

/// Loads ACPI tables from physical memory.
///
/// # Panic
///
/// Panics if the multiboot header doesn't contain any information about the ACPI tables, or if
/// the ACPI tables are invalid.
///
pub fn load_acpi_tables(
    multiboot_info: &multiboot2::BootInformation,
) -> acpi::AcpiTables<DummyHandler> {
    unsafe {
        let mut err = None;

        if let Some(rsdp_v2) = multiboot_info.rsdp_v2_tag() {
            match acpi::AcpiTables::from_rsdt(
                DummyHandler,
                rsdp_v2.revision(),
                rsdp_v2.xsdt_address(),
            ) {
                Ok(acpi) => return acpi,
                Err(e) => err = Some(e),
            }
        }

        if let Some(rsdp_v1) = multiboot_info.rsdp_v1_tag() {
            match acpi::AcpiTables::from_rsdt(
                DummyHandler,
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

/// Implementation of the `AcpiHandler` trait that is responsible for mapping physical memory
/// into virtual memory.
///
/// We use identity mapping over the whole address space, therefore this is a dummy.
#[derive(Debug, Clone)]
pub struct DummyHandler;
impl acpi::AcpiHandler for DummyHandler {
    unsafe fn map_physical_region<T>(
        &self,
        addr: usize,
        size: usize,
    ) -> acpi::PhysicalMapping<DummyHandler, T> {
        acpi::PhysicalMapping {
            physical_start: addr,
            virtual_start: NonNull::new(addr as *mut _).unwrap(),
            region_length: size,
            mapped_length: size,
            handler: self.clone(),
        }
    }

    fn unmap_physical_region<T>(&self, _: &acpi::PhysicalMapping<DummyHandler, T>) {}
}
