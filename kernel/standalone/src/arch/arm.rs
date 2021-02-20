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

use alloc::sync::Arc;
use core::{convert::TryFrom as _, fmt, iter, num::NonZeroU32, pin::Pin};
use futures::prelude::*;
use redshirt_kernel_log_interface::ffi::{KernelLogMethod, UartAccess, UartInfo};

#[cfg(target_arch = "aarch64")]
pub use time_aarch64 as time;
#[cfg(target_arch = "arm")]
pub use time_arm as time;

pub mod executor;
pub mod log;
pub mod time_aarch64;
pub mod time_arm;

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

            use alloc::sync::Arc;
            use core::{convert::TryFrom as _, fmt::Write as _, iter, num::NonZeroU32, pin::Pin};
            use $crate::futures::prelude::*;
            use $crate::klog::KLogger;
            use $crate::redshirt_kernel_log_interface::ffi::{KernelLogMethod, UartInfo};

            /// This is the main entry point of the kernel for ARM 32bits architectures.
            #[cfg(target_arch = "arm")]
            #[export_name = "_start"]
            #[naked]
            unsafe extern "C" fn entry_point_arm32() -> ! {
                // TODO: always fails :-/
                /*#[cfg(not(any(target_feature = "armv7-a", target_feature = "armv7-r")))]
                compile_error!("The ARMv7-A or ARMv7-R instruction sets must be enabled");*/

                // Detect which CPU we are.
                //
                // See sections B4.1.106 and B6.1.67 of the ARM® Architecture Reference Manual
                // (ARMv7-A and ARMv7-R edition).
                //
                // This is specific to ARMv7-A and ARMv7-R, hence the compile_error! above.
                asm!(
                    "
                    mrc p15, 0, r5, c0, c0, 5
                    and r5, r5, #3
                    cmp r5, #0
                    bne {}
                    ",
                    sym halt,
                    out("r3") _, out("r5") _,
                    options(nomem, nostack, preserves_flags)
                );

                // Only one CPU reaches here.

                // Zero the memory requested to be zero'ed.
                // TODO: that's illegal ; naked functions must only contain an asm! block (for good reasons)
                let mut ptr = &mut $memory_zeroing_start as *mut u8;
                while ptr < &mut $memory_zeroing_end as *mut u8 {
                    ptr.write_volatile(0);
                    ptr = ptr.add(1);
                }

                // Set up the stack and jump to the entry point.
                asm!("
                    .comm stack, 0x400000, 8
                    ldr sp, =stack+0x400000
                    b {}
                    ",
                    sym cpu_enter,
                    options(noreturn)
                )
            }

            /// This is the main entry point of the kernel for ARM 64bits architectures.
            #[cfg(target_arch = "aarch64")]
            #[export_name = "_start"]
            #[naked]
            unsafe extern "C" fn entry_point_arm64() -> ! {
                // TODO: review this
                asm!(
                    "
                        mrs x6, MPIDR_EL1
                        and x6, x6, #0x3
                        cbz x6, L0
                        b {}
                    L0: nop
                        ",
                    sym halt,
                    out("x6") _,
                    options(nomem, nostack)
                );

                // Only one CPU reaches here.

                // Zero the memory requested to be zero'ed.
                // TODO: that's illegal ; naked functions must only contain an asm! block (for good reasons)
                let mut ptr = &mut $memory_zeroing_start as *mut u8;
                while ptr < &mut $memory_zeroing_end as *mut u8 {
                    ptr.write_volatile(0);
                    ptr = ptr.add(1);
                }

                // Set up the stack and jump to `cpu_enter`.
        asm!(
                    "
                        .comm stack, 0x400000, 8
                        ldr x5, =stack+0x400000
                        mov sp, x5
                        b {}
                ", sym cpu_enter, options(noreturn))
            }

            /// Main Rust entry point.
            #[no_mangle]
            unsafe fn cpu_enter() -> ! {
                // Initialize the logging system.
                $crate::arch::arm::log::PANIC_LOGGER.set_method(KernelLogMethod {
                    enabled: true,
                    framebuffer: None,
                    uart: Some($crate::arch::arm::init_uart()),
                });

                // TODO: RAM starts at 0, but we start later to avoid the kernel
                // TODO: make this is a cleaner way
                $crate::mem_alloc::initialize(iter::once(0xa000000..0x40000000));

                let time = $crate::arch::arm::time::TimeControl::init();

                writeln!(
                    $crate::arch::arm::log::PANIC_LOGGER.log_printer(),
                    "[boot] boot successful"
                )
                .unwrap();

                let platform = Arc::pin($crate::arch::PlatformSpecific::from(
                    $crate::arch::arm::PlatformSpecificImpl { time },
                ));
                $crate::arch::arm::executor::block_on($entry(platform))
            }

            #[naked]
            fn halt() -> ! {
                unsafe {
                    loop {
                        asm!("wfe", options(nomem, nostack, preserves_flags));
                    }
                }
            }
        };
    };
}

/// Implementation of [`PlatformSpecific`].
#[doc(hidden)]
pub struct PlatformSpecificImpl {
    #[doc(hidden)]
    pub time: Arc<time::TimeControl>,
}

impl From<PlatformSpecificImpl> for super::PlatformSpecific {
    fn from(ps: PlatformSpecificImpl) -> Self {
        Self(ps)
    }
}

impl PlatformSpecificImpl {
    pub fn num_cpus(self: Pin<&Self>) -> NonZeroU32 {
        NonZeroU32::new(1).unwrap()
    }

    pub fn monotonic_clock(self: Pin<&Self>) -> u128 {
        self.time.monotonic_clock()
    }

    pub fn timer(self: Pin<&Self>, deadline: u128) -> TimerFuture {
        self.time.timer(deadline)
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

pub type TimerFuture = time::TimerFuture;
pub type IrqFuture = future::Pending<()>;

const GPIO_BASE: usize = 0x3F200000;
const UART0_BASE: usize = 0x3F201000;

#[doc(hidden)]
pub fn init_uart() -> UartInfo {
    unsafe {
        ((UART0_BASE + 0x30) as *mut u32).write_volatile(0x0);
        ((GPIO_BASE + 0x94) as *mut u32).write_volatile(0x0);
        delay(150);

        ((GPIO_BASE + 0x98) as *mut u32).write_volatile((1 << 14) | (1 << 15));
        delay(150);

        ((GPIO_BASE + 0x98) as *mut u32).write_volatile(0x0);

        ((UART0_BASE + 0x44) as *mut u32).write_volatile(0x7FF);

        ((UART0_BASE + 0x24) as *mut u32).write_volatile(1);
        ((UART0_BASE + 0x28) as *mut u32).write_volatile(40);

        ((UART0_BASE + 0x2C) as *mut u32).write_volatile((1 << 4) | (1 << 5) | (1 << 6));

        ((UART0_BASE + 0x38) as *mut u32).write_volatile(
            (1 << 1) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 10),
        );

        ((UART0_BASE + 0x30) as *mut u32).write_volatile((1 << 0) | (1 << 8) | (1 << 9));

        UartInfo {
            wait_address: UartAccess::MemoryMappedU32(u64::try_from(UART0_BASE + 0x18).unwrap()),
            wait_mask: 1 << 5,
            wait_compare_equal_if_ready: 0,
            write_address: UartAccess::MemoryMappedU32(u64::try_from(UART0_BASE + 0x0).unwrap()),
        }
    }
}

fn delay(count: i32) {
    unsafe {
        for _ in 0..count {
            asm!("nop", options(nostack, nomem, preserves_flags));
        }
    }
}
