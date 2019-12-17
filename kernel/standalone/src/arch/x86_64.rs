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

#![cfg(target_arch = "x86_64")]

use core::{convert::TryFrom as _, ops::Range};
use x86_64::registers::model_specific::Msr;
use x86_64::structures::port::{PortRead as _, PortWrite as _};

mod acpi;
mod boot_link;
mod interrupts;

/// Called by `boot.S` after basic set up has been performed.
///
/// When this function is called, a stack has been set up and as much memory space as possible has
/// been identity-mapped (i.e. the virtual memory is equal to the physical memory).
///
/// Since the kernel was loaded by a multiboot2 bootloader, the first parameter is the memory
/// address of the multiboot header.
#[no_mangle]
extern "C" fn after_boot(multiboot_header: usize) -> ! {
    unsafe {
        let multiboot_info = multiboot2::load(multiboot_header);

        crate::mem_alloc::initialize(find_free_memory_range(&multiboot_info));

        // TODO: panics in BOCHS
        //let acpi = acpi::load_acpi_tables(&multiboot_info);

        init_pic_apic();
        interrupts::init();

        let kernel = crate::kernel::Kernel::init(crate::kernel::KernelConfig {
            num_cpus: 1,
            ..Default::default()
        });

        kernel.run()
    }
}

// TODO: define the semantics of that
pub fn halt() -> ! {
    loop {
        unsafe { x86::halt() }
    }
}

/// Reads the boot information and find a memory range that can be used as a heap.
///
/// # Panic
///
/// Panics if the information is wrong or if there isn't enough information available.
///
fn find_free_memory_range(multiboot_info: &multiboot2::BootInformation) -> Range<usize> {
    let mem_map = multiboot_info.memory_map_tag().unwrap();
    let elf_sections = multiboot_info.elf_sections_tag().unwrap();

    // TODO: we choose the largest area, as we have no way to use multiple areas in
    // the allocator
    let area = mem_map.memory_areas().max_by_key(|mem| mem.size()).unwrap();
    let mut area_start = area.start_address();
    let mut area_end = area.end_address();
    debug_assert!(area_start <= area_end);

    // The kernel has probably been loaded into RAM, so we have to remove ELF sections
    // from the portion of memory that we use.
    for section in elf_sections.sections() {
        if section.start_address() >= area_start && section.end_address() <= area_end {
            /*         ↓ section_start    section_end ↓
               ==================================================
                  ↑ area_start                      area_end ↑
            */
            let off_bef = section.start_address() - area_start;
            let off_aft = area_end - section.end_address();
            if off_bef > off_aft {
                area_end = section.start_address();
            } else {
                area_start = section.end_address();
            }
        } else if section.start_address() < area_start && section.end_address() > area_end {
            /*    ↓ section_start             section_end ↓
               ==================================================
                       ↑ area_start         area_end ↑
            */
            // We have no memory available!
            panic!()
        } else if section.start_address() <= area_start && section.end_address() > area_start {
            /*    ↓ section_start     section_end ↓
               ==================================================
                       ↑ area_start                 area_end ↑
            */
            area_start = section.end_address();
        } else if section.start_address() < area_end && section.end_address() >= area_end {
            /*         ↓ section_start      section_end ↓
               ==================================================
                  ↑ area_start         area_end ↑
            */
            area_end = section.start_address();
        }
    }

    let area_start = usize::try_from(area_start).unwrap();
    let area_end = usize::try_from(area_end).unwrap();
    area_start..area_end
}

unsafe fn init_pic_apic() {
    // Remap and disable the PIC.
    //
    // The PIC (Programmable Interrupt Controller) is the old chip responsible for triggering
    // on the CPU interrupts coming from the hardware.
    //
    // Because of poor design decisions, it will by default trigger interrupts 0 to 15 on the CPU,
    // which are normally reserved for software-related concerns. For example, the timer will by
    // default trigger interrupt 8, which is also the double fault exception handler.
    //
    // In order to solve this issue, one has to reconfigure the PIC in order to make it trigger
    // interrupts between 32 and 47 rather than 0 to 15.
    //
    // Note that this code disables the PIC altogether. Despite the PIC being disabled, it is
    // still possible to receive spurious interrupts. Hence the remapping.
    u8::write_to_port(0xa1, 0xff);
    u8::write_to_port(0x21, 0xff);
    u8::write_to_port(0x20, 0x10 | 0x01);
    u8::write_to_port(0xa0, 0x10 | 0x01);
    u8::write_to_port(0x21, 0x20);
    u8::write_to_port(0xa1, 0x28);
    u8::write_to_port(0x21, 4);
    u8::write_to_port(0xa1, 2);
    u8::write_to_port(0x21, 0x01);
    u8::write_to_port(0xa1, 0x01);
    u8::write_to_port(0xa1, 0xff);
    u8::write_to_port(0x21, 0xff);

    // Set up the APIC.
    let apic_base_addr = {
        const APIC_BASE_MSR: Msr = Msr::new(0x1b);
        let base_addr = APIC_BASE_MSR.read() & !0xfff;
        APIC_BASE_MSR.write(base_addr | 0x800); // Enable the APIC.
        base_addr
    };

    // Enable spurious interrupts.
    {
        let svr_addr = usize::try_from(apic_base_addr + 0xf0).unwrap() as *mut u32;
        let val = svr_addr.read_volatile();
        svr_addr.write_volatile(val | 0x100); // Enable spurious interrupts.
    }
}

pub unsafe fn write_port_u8(port: u32, data: u8) {
    if let Ok(port) = u16::try_from(port) {
        u8::write_to_port(port, data);
    }
}

pub unsafe fn write_port_u16(port: u32, data: u16) {
    if let Ok(port) = u16::try_from(port) {
        u16::write_to_port(port, data);
    }
}

pub unsafe fn write_port_u32(port: u32, data: u32) {
    if let Ok(port) = u16::try_from(port) {
        u32::write_to_port(port, data);
    }
}

pub unsafe fn read_port_u8(port: u32) -> u8 {
    if let Ok(port) = u16::try_from(port) {
        u8::read_from_port(port)
    } else {
        0
    }
}

pub unsafe fn read_port_u16(port: u32) -> u16 {
    if let Ok(port) = u16::try_from(port) {
        u16::read_from_port(port)
    } else {
        0
    }
}

pub unsafe fn read_port_u32(port: u32) -> u32 {
    if let Ok(port) = u16::try_from(port) {
        u32::read_from_port(port)
    } else {
        0
    }
}
