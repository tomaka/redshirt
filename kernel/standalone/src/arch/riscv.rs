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

use crate::arch::{PlatformSpecific, PortErr};
use crate::klog::KLogger;

use core::{fmt, iter, num::NonZeroU32, pin::Pin};
use futures::prelude::*;
use redshirt_kernel_log_interface::ffi::{KernelLogMethod, UartInfo};

// Modules that are used by the macro must be public, but their content isn't meant to be used
// apart from the macro.
#[doc(hidden)]
pub mod executor;
#[doc(hidden)]
pub mod interrupts;
#[doc(hidden)]
pub mod log;

mod misc;

#[macro_export]
macro_rules! __gen_boot {
    (
        entry: $entry:path,
        memory_zeroing_start: $memory_zeroing_start:path,
        memory_zeroing_end: $memory_zeroing_end:path,
    ) => {
        const _: () = {
            extern crate alloc;

            use $crate::arch::{PlatformSpecific, PortErr};
            use $crate::arch::riscv::*;
            use $crate::klog::KLogger;

            use alloc::sync::Arc;
            use core::{arch::asm, fmt::Write as _, iter, num::NonZeroU32, pin::Pin};
            use $crate::futures::prelude::*;
            use $crate::redshirt_kernel_log_interface::ffi::{KernelLogMethod, UartAccess, UartInfo};

            /// This is the main entry point of the kernel for RISC-V architectures.
            #[naked]
            #[export_name = "_start"]
            unsafe extern "C" fn entry_point() {
                asm!(r#"
                    // Disable interrupts and clear pending interrupts.
                    csrw mie, 0
                    csrw mip, 0

                    // TODO: ???
                    .option push
                    .option norelax
                    la gp, __global_pointer$
                    .option pop

                    // Zero the memory requested to be zero'ed.
                    la a0, {memory_zeroing_start}
                    la a1, {memory_zeroing_end}
                .L0:sb zero, 0(a0)
                    addi a0, a0, 1
                    bltu a0, a1, .L0

                    // Set up the stack.
                    // TODO: make stack size configurable
                    // TODO: we don't have any stack protection in place
                    .comm stack, 0x2000, 8
                    la sp, stack
                    li t0, 0x2000
                    add sp, sp, t0
                    add fp, sp, zero

                    j {after_boot}
                "#,
                    memory_zeroing_start = sym $memory_zeroing_start,
                    memory_zeroing_end = sym $memory_zeroing_end,
                    after_boot = sym after_boot,
                    options(noreturn));
            }

            /// Main Rust entry point.
            unsafe fn after_boot() -> ! {
                // Initialize the logging system.
                log::PANIC_LOGGER.set_method(KernelLogMethod {
                    enabled: true,
                    framebuffer: None,
                    uart: Some(init_uart()),
                });

                // Initialize the memory allocator.
                // TODO: make this is a cleaner way; this is specific to the hifive
                $crate::mem_alloc::initialize(iter::once({
                    let free_mem_start = &$memory_zeroing_end as *const u8 as usize;
                    let ram_end = 0x80000000 + 16 * 1024;
                    free_mem_start..ram_end
                }));

                // Initialize interrupts.
                let _interrupts = interrupts::init();

                writeln!(log::PANIC_LOGGER.log_printer(), "[boot] boot successful").unwrap();

                // TODO: there's a stack overflow in practice when we call `kernel.run()`; the interrupt
                // handler fails to show that because it uses the stack
                panic!("We pre-emptively panic because running the kernel is known to overflow the stack");

                // Call the entry point specified by the user of the macro.
                // `` is used in order to jump out of the `__gen_boot` macro.
                let platform_specific = Arc::pin(PlatformSpecific::from(PlatformSpecificImpl {}));
                executor::block_on($entry(platform_specific))
            }

            // TODO: this is architecture-specific and very hacky
            fn init_uart() -> UartInfo {
                unsafe {
                    let prci_hfrosccfg = (0x10008000 as *mut u32).read_volatile();
                    (0x10008000 as *mut u32).write_volatile(prci_hfrosccfg | (1 << 30));

                    let prci_pllcfg = (0x10008008 as *mut u32).read_volatile();
                    (0x10008008 as *mut u32).write_volatile(prci_pllcfg | (1 << 18) | (1 << 17));
                    let prci_pllcfg = (0x10008008 as *mut u32).read_volatile();
                    (0x10008008 as *mut u32).write_volatile(prci_pllcfg | (1 << 16));

                    let prci_hfrosccfg = (0x10008000 as *mut u32).read_volatile();
                    (0x10008000 as *mut u32).write_volatile(prci_hfrosccfg & !(1 << 30));

                    let gpio_iof_sel = (0x1001203c as *mut u32).read_volatile();
                    (0x1001203c as *mut u32).write_volatile(gpio_iof_sel & !0x00030000);

                    let gpio_iof_en = (0x10012038 as *mut u32).read_volatile();
                    (0x10012038 as *mut u32).write_volatile(gpio_iof_en | 0x00030000);

                    (0x10013018 as *mut u32).write_volatile(138);

                    let uart_reg_tx_ctrl = (0x10013008 as *mut u32).read_volatile();
                    (0x10013008 as *mut u32).write_volatile(uart_reg_tx_ctrl | 1);

                    UartInfo {
                        wait_address: UartAccess::MemoryMappedU32(0x10013000),
                        wait_mask: 0x80000000,
                        wait_compare_equal_if_ready: 0,
                        write_address: UartAccess::MemoryMappedU32(0x10013000),
                    }
                }
            }
        };
    }
}

/// Implementation of [`PlatformSpecific`].
pub struct PlatformSpecificImpl {}

impl From<PlatformSpecificImpl> for super::PlatformSpecific {
    fn from(ps: PlatformSpecificImpl) -> Self {
        Self(ps)
    }
}

impl PlatformSpecificImpl {
    pub fn num_cpus(self: Pin<&Self>) -> NonZeroU32 {
        // TODO:
        NonZeroU32::new(1).unwrap()
    }

    #[cfg(target_pointer_width = "32")]
    pub fn monotonic_clock(self: Pin<&Self>) -> u128 {
        // TODO: unit is probably the wrong unit; we're supposed to return nanoseconds
        // TODO: this is only supported in the "I" version of RISC-V; check that
        unsafe {
            // Because we can't read the entire register atomically, we have to carefully handle
            // the possibility of an overflow of the lower bits during the reads. This is also
            // shown as the example that the manual uses for reading the clock on RV32I.
            let val = loop {
                let lo: u32;
                let hi1: u32;
                let hi2: u32;

                // Note that we put all three instructions in the same `asm!`, to prevent the
                // compiler from possibly reordering them.
                asm!("rdtimeh {} ; rdtime {} ; rdtimeh {}", out(reg) hi1, out(reg) lo, out(reg) hi2);

                if hi1 == hi2 {
                    break (u64::from(hi1) << 32) | u64::from(lo);
                }
            };

            u128::from(val)
        }
    }

    #[cfg(target_pointer_width = "64")]
    pub fn monotonic_clock(self: Pin<&Self>) -> u128 {
        // TODO: unit is probably the wrong unit; we're supposed to return nanoseconds
        // TODO: this is only supported in the "I" version of RISC-V; check that
        unsafe {
            let val: u64;
            asm!("rdtime {}", out(reg) reg);
            u128::from(val)
        }
    }

    pub fn timer(self: Pin<&Self>, _deadline: u128) -> TimerFuture {
        todo!()
    }

    pub fn next_irq(self: Pin<&Self>) -> IrqFuture {
        future::pending()
    }

    pub fn write_log(&self, message: &str) {
        fmt::Write::write_str(&mut log::PANIC_LOGGER.log_printer(), message).unwrap();
    }

    pub fn set_logger_method(&self, method: KernelLogMethod) {
        log::PANIC_LOGGER.set_method(method);
    }

    pub unsafe fn write_port_u8(self: Pin<&Self>, _: u32, _: u8) -> Result<(), PortErr> {
        Err(PortErr::Unsupported)
    }

    pub unsafe fn write_port_u16(self: Pin<&Self>, _: u32, _: u16) -> Result<(), PortErr> {
        Err(PortErr::Unsupported)
    }

    pub unsafe fn write_port_u32(self: Pin<&Self>, _: u32, _: u32) -> Result<(), PortErr> {
        Err(PortErr::Unsupported)
    }

    pub unsafe fn read_port_u8(self: Pin<&Self>, _: u32) -> Result<u8, PortErr> {
        Err(PortErr::Unsupported)
    }

    pub unsafe fn read_port_u16(self: Pin<&Self>, _: u32) -> Result<u16, PortErr> {
        Err(PortErr::Unsupported)
    }

    pub unsafe fn read_port_u32(self: Pin<&Self>, _: u32) -> Result<u32, PortErr> {
        Err(PortErr::Unsupported)
    }
}

pub type TimerFuture = future::Pending<()>;
pub type IrqFuture = future::Pending<()>;
