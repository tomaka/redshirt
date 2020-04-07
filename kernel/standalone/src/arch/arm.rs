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

#![cfg(any(target_arch = "arm", target_arch = "aarch64"))]

use crate::arch::{PlatformSpecific, PortErr};

use alloc::sync::Arc;
use core::{iter, num::NonZeroU32, pin::Pin};
use futures::prelude::*;

#[cfg(target_arch = "aarch64")]
use time_aarch64 as time;
#[cfg(target_arch = "arm")]
use time_arm as time;

mod executor;
mod misc;
mod panic;
mod time_aarch64;
mod time_arm;

/// This is the main entry point of the kernel for ARM 32bits architectures.
#[cfg(target_arch = "arm")]
#[no_mangle]
#[naked]
unsafe extern "C" fn _start() -> ! {
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
        r#"
    mrc p15, 0, r5, c0, c0, 5
    and r5, r5, #3
    cmp r5, #0
    bne halt
    "#::::"volatile");

    // Only one CPU reaches here.

    // Zero the BSS segment.
    // TODO: we pray here that the compiler doesn't use the stack
    let mut ptr = &__bss_start as *mut u8;
    while ptr < &__bss_end as *mut u8 {
        ptr.write_volatile(0);
        ptr = ptr.add(1);
    }

    // Set up the stack.
    asm!(r#"
    .comm stack, 0x400000, 8
    ldr sp, =stack+0x400000"#:::"memory":"volatile");

    asm!(r#"b cpu_enter"#:::"volatile");
    core::hint::unreachable_unchecked()
}

/// This is the main entry point of the kernel for ARM 64bits architectures.
#[cfg(target_arch = "aarch64")]
#[no_mangle]
#[naked]
unsafe extern "C" fn _start() -> ! {
    // TODO: review this
    asm!(r#"
    mrs x6, MPIDR_EL1
    and x6, x6, #0x3
    cbz x6, L0
    b halt
L0: nop
    "#::::"volatile");

    // Only one CPU reaches here.

    // Zero the BSS segment.
    // TODO: we pray here that the compiler doesn't use the stack
    let mut ptr = &__bss_start as *mut u8;
    while ptr < &__bss_end as *mut u8 {
        ptr.write_volatile(0);
        ptr = ptr.add(1);
    }

    // Set up the stack.
    asm!(r#"
    .comm stack, 0x400000, 8
    ldr x5, =stack+0x400000; mov sp, x5"#:::"memory":"volatile");

    asm!(r#"b cpu_enter"#:::"volatile");
    core::hint::unreachable_unchecked()
}

extern "C" {
    static mut __bss_start: u8;
    static mut __bss_end: u8;
}

/// Main Rust entry point.
#[no_mangle]
fn cpu_enter() -> ! {
    unsafe {
        // TODO: RAM starts at 0, but we start later to avoid the kernel
        // TODO: make this is a cleaner way
        crate::mem_alloc::initialize(iter::once(0xa000000..0x40000000));
    }

    let time = unsafe { time::TimeControl::init() };

    let kernel = crate::kernel::Kernel::init(PlatformSpecificImpl { time });
    executor::block_on(kernel.run())
}

/// Implementation of [`PlatformSpecific`].
struct PlatformSpecificImpl {
    time: Arc<time::TimeControl>,
}

impl PlatformSpecific for PlatformSpecificImpl {
    type TimerFuture = time::TimerFuture;

    fn num_cpus(self: Pin<&Self>) -> NonZeroU32 {
        NonZeroU32::new(1).unwrap()
    }

    fn monotonic_clock(self: Pin<&Self>) -> u128 {
        self.time.monotonic_clock()
    }

    fn timer(self: Pin<&Self>, deadline: u128) -> Self::TimerFuture {
        self.time.timer(deadline)
    }

    unsafe fn write_port_u8(self: Pin<&Self>, _: u32, _: u8) -> Result<(), PortErr> {
        Err(PortErr::Unsupported)
    }

    unsafe fn write_port_u16(self: Pin<&Self>, _: u32, _: u16) -> Result<(), PortErr> {
        Err(PortErr::Unsupported)
    }

    unsafe fn write_port_u32(self: Pin<&Self>, _: u32, _: u32) -> Result<(), PortErr> {
        Err(PortErr::Unsupported)
    }

    unsafe fn read_port_u8(self: Pin<&Self>, _: u32) -> Result<u8, PortErr> {
        Err(PortErr::Unsupported)
    }

    unsafe fn read_port_u16(self: Pin<&Self>, _: u32) -> Result<u16, PortErr> {
        Err(PortErr::Unsupported)
    }

    unsafe fn read_port_u32(self: Pin<&Self>, _: u32) -> Result<u32, PortErr> {
        Err(PortErr::Unsupported)
    }
}

// TODO: no_mangle and naked because it's called at initialization; attributes should eventually be removed
#[no_mangle]
#[naked]
fn halt() -> ! {
    unsafe {
        loop {
            asm!(r#"wfe"#);
        }
    }
}
