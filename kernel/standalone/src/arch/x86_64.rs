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

use crate::arch::{PlatformSpecific, PortErr};
use crate::klog::KLogger;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{
    convert::TryFrom as _, fmt::Write as _, iter, num::NonZeroU32, ops::Range, pin::Pin,
    time::Duration,
};
use futures::channel::oneshot;
use redshirt_kernel_log_interface::ffi::{FramebufferFormat, FramebufferInfo, KernelLogMethod};
use x86_64::structures::port::{PortRead as _, PortWrite as _};

mod acpi;
mod ap_boot;
mod apic;
mod boot;
mod executor;
mod interrupts;
mod panic;
mod pit;

const DEFAULT_LOG_METHOD: KernelLogMethod = KernelLogMethod {
    enabled: true,
    framebuffer: Some(FramebufferInfo {
        address: 0xb8000,
        width: 80,
        height: 25,
        pitch: 160,
        bytes_per_character: 2,
        format: FramebufferFormat::Text,
    }),
    uart: None,
};

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
/// `multiboot_info` must be a valid memory address that contains valid information.
///
#[no_mangle]
unsafe extern "C" fn after_boot(multiboot_info: usize) -> ! {
    let multiboot_info = multiboot2::load(multiboot_info);

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

    // Now that we have a memory allocator, initialize the logging system .
    let logger = Arc::new(KLogger::new({
        if let Some(fb_info) = multiboot_info.framebuffer_tag() {
            KernelLogMethod {
                enabled: true,
                framebuffer: Some(FramebufferInfo {
                    address: fb_info.address,
                    width: fb_info.width,
                    height: fb_info.height,
                    pitch: u64::from(fb_info.pitch),
                    bytes_per_character: fb_info.bpp / 8,
                    format: match fb_info.buffer_type {
                        multiboot2::FramebufferType::Text => FramebufferFormat::Text,
                        multiboot2::FramebufferType::Indexed { .. } => FramebufferFormat::Rgb {
                            // FIXME: that is completely wrong
                            red_size: 8,
                            red_position: 0,
                            green_size: 8,
                            green_position: 16,
                            blue_size: 8,
                            blue_position: 24,
                        },
                        multiboot2::FramebufferType::RGB { red, green, blue } => {
                            FramebufferFormat::Rgb {
                                red_size: red.size,
                                red_position: red.position,
                                green_size: green.size,
                                green_position: green.position,
                                blue_size: blue.size,
                                blue_position: blue.position,
                            }
                        }
                    },
                }),
                uart: None,
            }
        } else {
            DEFAULT_LOG_METHOD.clone()
        }
    }));

    // If a panic happens, we want it to use the logging system we just created.
    panic::set_logger(logger.clone());

    // The first thing that gets executed when a x86 or x86_64 machine starts up is the
    // motherboard's firmware. Before giving control to the operating system, this firmware writes
    // into memory a set of data called the **ACPI tables**.
    // It then (indirectly) passes the memory address of this table to the operating system. This
    // is part of [the UEFI standard](https://en.wikipedia.org/wiki/UEFI).
    //
    // However, this code is not loaded directly by the firmware but rather by a bootloader. This
    // bootloader must save the information about the ACPI tables and propagate it as part of the
    // multiboot2 header passed to the operating system.
    // TODO: remove these tables from the memory ranges used as heap? `acpi_tables` is a copy of
    // the table, so once we are past this line there's no problem anymore. But in theory,
    // the `acpi_tables` variable might allocate over the actual ACPI tables.
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

    writeln!(logger.log_printer(), "initializing associated processors").unwrap();
    for ap in acpi_tables.application_processors.iter() {
        debug_assert!(ap.is_ap);
        // It is possible for some associated processors to be in a disabled state, in which case
        // they **must not** be started. This is generally the case of defective processors.
        if ap.state != ::acpi::ProcessorState::WaitingForSipi {
            continue;
        }

        let (kernel_tx, kernel_rx) = oneshot::channel::<Arc<crate::kernel::Kernel<_>>>();

        let ap_boot_result = ap_boot::boot_associated_processor(
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

        match ap_boot_result {
            Ok(()) => kernel_channels.push(kernel_tx),
            Err(err) =>
                writeln!(logger.log_printer(), "error while initializing AP#{}: {}", ap.processor_uid, err).unwrap()
        }
    }

    // Now that everything has been initialized and all the processors started, we can initialize
    // the kernel.
    let kernel = {
        let platform_specific = PlatformSpecificImpl {
            timers,
            num_cpus: NonZeroU32::new(
                u32::try_from(kernel_channels.len())
                    .unwrap()
                    .checked_add(1)
                    .unwrap(),
            )
            .unwrap(),
            logger: logger.clone(),
        };

        Arc::new(crate::kernel::Kernel::init(platform_specific))
    };

    writeln!(logger.log_printer(), "boot successful").unwrap();

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
        // Some parts of the memory have to be avoided, such as the kernel, non-RAM memory,
        // RAM that might contain important information, and so on.
        let to_avoid = {
            // TODO: for now, the code in boot.rs only maps the first 32GiB of memory. We avoid
            // anything above this limit
            //let unmapped = iter::once(0x2000000000 .. u64::max_value());
            // TODO: linked_list_allocator seems to misbehave when we use a lot of memory, so for
            // now we restrict ourselves to the first 2GiB.
            let unmapped = iter::once(0x80000000..u64::max_value());

            // We don't want to write over the kernel that has been loaded in memory.
            let elf = elf_sections
                .sections()
                .map(|s| s.start_address()..s.end_address());

            // We don't want to use the memory-mapped ROM or video memory.
            let rom_video_ram = iter::once(0xa0000..0xfffff);

            // Some areas in the first megabyte were used during the booting process. This
            // includes the 16bits interrupt vector table and the memory used by the BIOS to keep
            // track of its state.
            // Note that since we have total control over the hardware there is no fundamental
            // reason to not overwrite these areas. In practice, however, there are situations
            // where we would like to read these information later (for example if a VBE driver
            // wants to access the content of the video BIOS).
            let important_info = iter::once(0..0x500).chain(iter::once(0x80000..0xa0000));

            // Avoid writing over the multiboot header.
            let multiboot = iter::once(
                u64::try_from(multiboot_info.start_address()).unwrap()
                    ..u64::try_from(multiboot_info.end_address()).unwrap(),
            );

            // Apart from the areas above, there are other areas that we want to avoid, in
            // particular memory-mapped hardware. We trust the multiboot information to not
            // include them.
            elf.chain(rom_video_ram)
                .chain(important_info)
                .chain(multiboot)
                .chain(unmapped)
        };

        let mut area_start = area.start_address();
        let mut area_end = area.end_address();
        debug_assert!(area_start <= area_end);

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
    num_cpus: NonZeroU32,
    logger: Arc<KLogger>,
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

    fn write_log(&self, message: &str) {
        writeln!(self.logger.log_printer(), "{}", message).unwrap();
    }

    fn set_logger_method(&self, method: KernelLogMethod) {
        self.logger.set_method(method)
    }

    unsafe fn write_port_u8(self: Pin<&Self>, port: u32, data: u8) -> Result<(), PortErr> {
        if let Ok(port) = u16::try_from(port) {
            u8::write_to_port(port, data);
            Ok(())
        } else {
            Err(PortErr::OutOfRange)
        }
    }

    unsafe fn write_port_u16(self: Pin<&Self>, port: u32, data: u16) -> Result<(), PortErr> {
        if let Ok(port) = u16::try_from(port) {
            u16::write_to_port(port, data);
            Ok(())
        } else {
            Err(PortErr::OutOfRange)
        }
    }

    unsafe fn write_port_u32(self: Pin<&Self>, port: u32, data: u32) -> Result<(), PortErr> {
        if let Ok(port) = u16::try_from(port) {
            u32::write_to_port(port, data);
            Ok(())
        } else {
            Err(PortErr::OutOfRange)
        }
    }

    unsafe fn read_port_u8(self: Pin<&Self>, port: u32) -> Result<u8, PortErr> {
        if let Ok(port) = u16::try_from(port) {
            Ok(u8::read_from_port(port))
        } else {
            Err(PortErr::OutOfRange)
        }
    }

    unsafe fn read_port_u16(self: Pin<&Self>, port: u32) -> Result<u16, PortErr> {
        if let Ok(port) = u16::try_from(port) {
            Ok(u16::read_from_port(port))
        } else {
            Err(PortErr::OutOfRange)
        }
    }

    unsafe fn read_port_u32(self: Pin<&Self>, port: u32) -> Result<u32, PortErr> {
        if let Ok(port) = u16::try_from(port) {
            Ok(u32::read_from_port(port))
        } else {
            Err(PortErr::OutOfRange)
        }
    }
}
