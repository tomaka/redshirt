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

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{
    convert::TryFrom as _, future::Future, iter, num::NonZeroU32, ops::Range, pin::Pin,
    time::Duration,
};
use futures::channel::oneshot;
use x86_64::structures::port::{PortRead as _, PortWrite as _};

mod acpi;
mod ap_boot;
mod apic;
mod boot_link;
mod executor;
mod interrupts;
mod panic;
mod pit;

/// Called by `boot.S` after basic set up has been performed.
///
/// When this function is called, a stack has been set up and as much memory space as possible has
/// been identity-mapped (i.e. the virtual memory is equal to the physical memory).
///
/// Since the kernel was loaded by a multiboot2 bootloader, the first parameter is the memory
/// address of the multiboot header.
///
/// # Safety
///
/// `multiboot_header` must be a valid memory address that contains valid information.
///
#[no_mangle]
unsafe extern "C" fn after_boot(multiboot_header: usize) -> ! {
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

    // The first thing that gets executed when a x86 or x86_64 machine starts up is the
    // motherboard's firmware. Before giving control to the operating system, this firmware writes
    // into memory a set of data called the **ACPI tables**.
    // It then (indirectly) passes the memory address of this table to the operating system. This
    // is part of [the UEFI standard](https://en.wikipedia.org/wiki/UEFI).
    //
    // However, this code is not loaded directly by the firmware but rather by a bootloader. This
    // bootloader must save the information about the ACPI tables and propagate it as part of the
    // multiboot2 header passed to the operating system.
    // TODO: panics in BOCHS
    let acpi_tables = acpi::load_acpi_tables(&multiboot_info);

    // The ACPI tables indicate us information about how to interface with the I/O APICs.
    // We use this information and initialize the I/O APICs.
    let mut io_apics = match &acpi_tables.interrupt_model {
        Some(::acpi::interrupt::InterruptModel::Apic(apic)) => {
            // The PIC is the legacy equivalent of I/O APICs.
            apic::pic::init_and_disable_pic();
            apic::io_apics::init_from_acpi(apic)
        }
        Some(_) => panic!("Legacy PIC mode not supported"),
        None => panic!("Interrupt model ACPI table not found"),
    };

    // We then initialize the local APIC.
    // `Box::leak` gives us a `&'static` reference to the object.
    let local_apics = Box::leak(Box::new(apic::local::init()));

    // Initialize an object that can execute futures between CPUs.
    let executor = Box::leak(Box::new(executor::Executor::new(&*local_apics)));

    // The PIT is an old mechanism for triggering interrupts after a certain delay.
    // Despite being old, it is still present on all hardware.
    let mut pit = pit::init_pit(&*local_apics, &mut io_apics);

    // Initialize interrupts so that all the elements initialize above function properly.
    // TODO: make this more fool-proof
    interrupts::load_idt();

    // Initialize the timers state machine.
    // This allows us to create `Future`s that resolve after a certain amount of time has passed.
    let timers = Box::leak(Box::new(apic::timers::init(
        local_apics,
        &*executor,
        &mut pit,
    )));

    // This code is only executed by the main processor of the machine, called the **boot
    // processor**. The other processors are called the **associated processors** and must be
    // manually started.

    // This Vec will contain one `oneshort::Sender<Arc<Kernel>>` for each associated processor
    // that has been started. Once the kernel is initialized, we send a reference-counted copy of
    // it to each sender.
    let mut kernel_channels = Vec::with_capacity(acpi_tables.application_processors.len());

    for ap in acpi_tables.application_processors.iter() {
        debug_assert!(ap.is_ap);
        // It is possible for some associated processors to be in a disabled state, in which case
        // they **must not** be started. This is generally the case of defective processors.
        if ap.state != ::acpi::ProcessorState::WaitingForSipi {
            continue;
        }

        let (kernel_tx, kernel_rx) = oneshot::channel::<Arc<crate::kernel::Kernel<_>>>();
        kernel_channels.push(kernel_tx);

        ap_boot::boot_associated_processor(
            &mut ap_boot_alloc,
            &*executor,
            &*local_apics,
            timers,
            apic::ApicId::from_unchecked(ap.local_apic_id),
            {
                let executor = &*executor;
                move || {
                    let kernel = executor.block_on(kernel_rx).unwrap();
                    // The `run()` method never returns.
                    executor.block_on(kernel.run())
                }
            },
        );
    }

    // Now that everything has been initialized and all the processors started, we can initialize
    // the kernel.
    let kernel = {
        let platform_specific = PlatformSpecificImpl {
            timers,
            executor: &*executor,
            local_apics,
            num_cpus: NonZeroU32::new(
                u32::try_from(kernel_channels.len())
                    .unwrap()
                    .checked_add(1)
                    .unwrap(),
            )
            .unwrap(),
        };

        Arc::new(crate::kernel::Kernel::init(platform_specific))
    };

    // Send an `Arc<Kernel>` to the other processors so that they can run it too.
    for tx in kernel_channels {
        if tx.send(kernel.clone()).is_err() {
            panic!();
        }
    }

    // Start the kernel on the boot processor too.
    // This function never returns.
    executor.block_on(kernel.run())
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

        // The kernel and various information about the system have been loaded into RAM, so we
        // have to remove all the sections we want to keep from the portions of memory that we
        // use.
        let to_avoid = {
            let elf = elf_sections
                .sections()
                .map(|s| s.start_address()..s.end_address());
            let multiboot = iter::once(
                u64::try_from(multiboot_info.start_address()).unwrap()
                    ..u64::try_from(multiboot_info.end_address()).unwrap(),
            );
            // TODO: ACPI tables
            // TODO: PCI stuff?
            // TODO: memory map stuff?
            elf.chain(multiboot)
        };

        for section in to_avoid {
            if section.start >= area_start && section.end <= area_end {
                /*         ↓ section_start    section_end ↓
                ==================================================
                    ↑ area_start                      area_end ↑
                */
                let off_bef = section.start - area_start;
                let off_aft = area_end - section.end;
                if off_bef > off_aft {
                    area_end = section.start;
                } else {
                    area_start = section.end;
                }
            } else if section.start < area_start && section.end > area_end {
                /*    ↓ section_start             section_end ↓
                ==================================================
                        ↑ area_start         area_end ↑
                */
                // We have no memory available!
                return None;
            } else if section.start <= area_start && section.end > area_start {
                /*    ↓ section_start     section_end ↓
                ==================================================
                        ↑ area_start                 area_end ↑
                */
                area_start = section.end;
            } else if section.start < area_end && section.end >= area_end {
                /*         ↓ section_start      section_end ↓
                ==================================================
                    ↑ area_start         area_end ↑
                */
                area_end = section.start;
            }
        }

        let area_start = usize::try_from(area_start).unwrap();
        let area_end = usize::try_from(area_end).unwrap();
        Some(area_start..area_end)
    })
}

/// Implementation of [`PlatformSpecific`].
struct PlatformSpecificImpl {
    timers: &'static apic::timers::Timers<'static>,
    local_apics: &'static apic::local::LocalApicsControl,
    executor: &'static executor::Executor,
    num_cpus: NonZeroU32,
}

impl PlatformSpecific for PlatformSpecificImpl {
    type TimerFuture = apic::timers::TimerFuture<'static>;

    fn num_cpus(self: Pin<&Self>) -> NonZeroU32 {
        self.num_cpus
    }

    fn monotonic_clock(self: Pin<&Self>) -> u128 {
        self.timers.monotonic_clock().as_nanos()
    }

    fn timer(self: Pin<&Self>, clock_value: u128) -> Self::TimerFuture {
        self.timers.register_tsc_timer({
            let secs = u64::try_from(clock_value / 1_000_000_000).unwrap_or(u64::max_value());
            let nanos = u32::try_from(clock_value % 1_000_000_000).unwrap();
            Duration::new(secs, nanos)
        })
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
