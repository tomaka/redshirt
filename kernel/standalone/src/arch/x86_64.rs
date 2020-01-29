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

#![cfg(target_arch = "x86_64")]

use crate::arch::PlatformSpecific;

use alloc::{sync::Arc, vec::Vec};
use core::{convert::TryFrom as _, future::Future, num::NonZeroU32, ops::Range, pin::Pin};
use futures::channel::oneshot;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::port::{PortRead as _, PortWrite as _};

mod acpi;
mod ap_boot;
mod apic;
mod boot_link;
mod executor;
mod interrupts;
mod panic;

/// Called by `boot.S` after basic set up has been performed.
///
/// When this function is called, a stack has been set up and as much memory space as possible has
/// been identity-mapped (i.e. the virtual memory is equal to the physical memory).
///
/// Since the kernel was loaded by a multiboot2 bootloader, the first parameter is the memory
/// address of the multiboot header.
#[no_mangle]
#[cold]
extern "C" fn after_boot(multiboot_header: usize) -> ! {
    unsafe {
        let multiboot_info = multiboot2::load(multiboot_header);

        // Initialization of the memory allocator.
        let mut ap_boot_alloc = {
            let mut ap_boot_alloc = None;
            // The associated processors (AP) boot code requires its own allocator. We take all
            // the free ranges reported by the multiboot header and pass them to the `ap_boot`
            // allocator initialization code so that it can filter out one that it needs.
            let remaining_ranges = ap_boot::filter_build_ap_boot_alloc(
                find_free_memory_ranges(&multiboot_info),
                &mut ap_boot_alloc,
            );

            // Pass the free remaining ranges to the main allocator of the kernel.
            crate::mem_alloc::initialize(remaining_ranges);

            match ap_boot_alloc {
                Some(b) => b,
                None => panic!("Couldn't find free memory range for the AP allocator"),
            }
        };

        // TODO: panics in BOCHS
        let acpi = acpi::load_acpi_tables(&multiboot_info);

        interrupts::init();

        let apic = apic::init();

        let mut kernel_channels = Vec::with_capacity(acpi.application_processors.len());

        for ap in acpi.application_processors.iter().take(1) {
            // TODO: remove take(1), but doesn't work if I do so
            debug_assert!(ap.is_ap);
            if ap.state != ::acpi::ProcessorState::WaitingForSipi {
                continue;
            }

            let (kernel_tx, kernel_rx) = oneshot::channel::<Arc<crate::kernel::Kernel<_>>>();
            kernel_channels.push(kernel_tx);
            let apic_clone = apic.clone();

            ap_boot::boot_associated_processor(
                &mut ap_boot_alloc,
                &apic,
                apic::ApicId::from_unchecked(ap.local_apic_id),
                move || {
                    let kernel = executor::block_on(&apic_clone, kernel_rx).unwrap();
                    kernel.run();
                },
            );
        }

        let kernel = {
            let platform_specific = PlatformSpecificImpl {
                apic,
                num_cpus: NonZeroU32::new(u32::try_from(kernel_channels.len()).unwrap().checked_add(1).unwrap()).unwrap(),
            };

            Arc::new(crate::kernel::Kernel::init(platform_specific))
        };

        for tx in kernel_channels {
            if tx.send(kernel.clone()).is_err() {
                panic!();
            }
        }

        kernel.run()
    }
}

/// Reads the boot information and find the memory ranges that can be used as a heap.
///
/// # Panic
///
/// Panics if the information is wrong or if there isn't enough information available.
///
fn find_free_memory_ranges<'a>(
    multiboot_info: &'a multiboot2::BootInformation,
) -> impl Iterator<Item = Range<usize>> + 'a {
    let mem_map = multiboot_info.memory_map_tag().unwrap();
    let elf_sections = multiboot_info.elf_sections_tag().unwrap();

    mem_map.memory_areas().filter_map(move |area| {
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
                return None;
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
        Some(area_start..area_end)
    })
}

/// Implementation of [`PlatformSpecific`].
struct PlatformSpecificImpl {
    apic: Arc<apic::ApicControl>,
    num_cpus: NonZeroU32,
}

impl PlatformSpecific for PlatformSpecificImpl {
    type TimerFuture = apic::TscTimerFuture;

    fn num_cpus(self: Pin<&Self>) -> NonZeroU32 {
        self.num_cpus
    }

    fn block_on<TRet>(self: Pin<&Self>, future: impl Future<Output = TRet>) -> TRet {
        executor::block_on(&self.apic, future)
    }

    fn monotonic_clock(self: Pin<&Self>) -> u128 {
        // TODO: wrong unit; these are not nanoseconds
        // TODO: maybe TSC not supported? move method to ApicControl instead?
        u128::from(unsafe { core::arch::x86_64::_rdtsc() })
    }

    fn timer(self: Pin<&Self>, clock_value: u128) -> Self::TimerFuture {
        let clock_value = u64::try_from(clock_value).unwrap_or(u64::max_value());
        self.apic.register_tsc_timer(clock_value)
    }

    unsafe fn write_port_u8(self: Pin<&Self>, port: u32, data: u8) -> Result<(), ()> {
        if let Ok(port) = u16::try_from(port) {
            u8::write_to_port(port, data);
            Ok(())
        } else {
            Err(())
        }
    }

    unsafe fn write_port_u16(self: Pin<&Self>, port: u32, data: u16) -> Result<(), ()> {
        if let Ok(port) = u16::try_from(port) {
            u16::write_to_port(port, data);
            Ok(())
        } else {
            Err(())
        }
    }

    unsafe fn write_port_u32(self: Pin<&Self>, port: u32, data: u32) -> Result<(), ()> {
        if let Ok(port) = u16::try_from(port) {
            u32::write_to_port(port, data);
            Ok(())
        } else {
            Err(())
        }
    }

    unsafe fn read_port_u8(self: Pin<&Self>, port: u32) -> Result<u8, ()> {
        if let Ok(port) = u16::try_from(port) {
            Ok(u8::read_from_port(port))
        } else {
            Err(())
        }
    }

    unsafe fn read_port_u16(self: Pin<&Self>, port: u32) -> Result<u16, ()> {
        if let Ok(port) = u16::try_from(port) {
            Ok(u16::read_from_port(port))
        } else {
            Err(())
        }
    }

    unsafe fn read_port_u32(self: Pin<&Self>, port: u32) -> Result<u32, ()> {
        if let Ok(port) = u16::try_from(port) {
            Ok(u32::read_from_port(port))
        } else {
            Err(())
        }
    }
}
