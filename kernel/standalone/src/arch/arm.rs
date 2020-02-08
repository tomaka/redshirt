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

use crate::arch::PlatformSpecific;

use core::{iter, num::NonZeroU32, pin::Pin};
use futures::prelude::*;

mod executor;
mod misc;
mod panic;

// TODO: always fails :-/
/*#[cfg(not(any(target_feature = "armv7-a", target_feature = "armv7-r")))]
compile_error!("The ARMv7-A or ARMv7-R instruction sets must be enabled");*/

/// This is the main entry point of the kernel for ARM architectures.
#[no_mangle]
#[naked]
unsafe extern "C" fn _start() -> ! {
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

    // Set up the stack.
    asm!(r#"
    .comm stack, 0x400000, 8
    ldr sp, =stack+0x400000"#:::"memory":"volatile");

    // On ARM platforms, the `r0`, `r1` and `r2` registers are used to pass the first three
    // parameters when calling a function.
    // Since we don't modify the values of these registers in this function, we can simply branch
    // to `cpu_enter`, and it will receive the same parameters as what the bootloader passed to
    // us.
    // TODO: to be honest, I'd prefer retreiving the values of r0, r1 and r2 in local variables
    // first, and then pass them to `cpu_enter` as parameters. In practice, though, I don't want
    // to deal with the syntax of `asm!`.
    asm!(r#"b cpu_enter"#:::"volatile");
    core::hint::unreachable_unchecked()
}

/// Main Rust entry point. The three parameters are the values of the `r0`, `r1` and `r2`
/// registers as they were when we entered the kernel.
#[no_mangle]
fn cpu_enter(_r0: u32, _r1: u32, _r2: u32) -> ! {
    unsafe {
        // TODO: RAM starts at 0, but we start later to avoid the kernel
        // TODO: make this is a cleaner way
        crate::mem_alloc::initialize(iter::once(0xa000000..0x40000000));
    }

    // TODO: The `r0`, `r1` and `r2` parameters are supposedly set by the bootloader, and `r2`
    // points either to ATAGS or a DTB (device tree) indicating what the hardware supports. This
    // is unfortunately not supported by QEMU as of the writing of this comment.

    // Initialize performance counters.
    // TODO: do that properly and well isolated
    // TODO: also, we just assume that counters are supported
    unsafe {
        asm!("mcr p15, 0, $0, c9, c12, 0"::"r"(0b111u32)::"volatile");
        asm!("mcr p15, 0, $0, c9, c12, 1"::"r"(0x8000000fu32)::"volatile");
        asm!("mcr p15, 0, $0, c9, c12, 3"::"r"(0x8000000fu32)::"volatile");
    }

    let kernel = crate::kernel::Kernel::init(PlatformSpecificImpl);
    kernel.run()
}

/// Implementation of [`PlatformSpecific`].
struct PlatformSpecificImpl;

impl PlatformSpecific for PlatformSpecificImpl {
    type TimerFuture = future::Pending<()>;

    fn num_cpus(self: Pin<&Self>) -> NonZeroU32 {
        NonZeroU32::new(1).unwrap()
    }

    fn block_on<TRet>(self: Pin<&Self>, future: impl Future<Output = TRet>) -> TRet {
        executor::block_on(future)
    }

    fn monotonic_clock(self: Pin<&Self>) -> u128 {
        // TODO: implement correctly
        0xdeadbeefu128
    }

    fn timer(self: Pin<&Self>, clock_value: u128) -> Self::TimerFuture {
        future::pending()
    }

    unsafe fn write_port_u8(self: Pin<&Self>, _: u32, _: u8) -> Result<(), ()> {
        Err(())
    }

    unsafe fn write_port_u16(self: Pin<&Self>, _: u32, _: u16) -> Result<(), ()> {
        Err(())
    }

    unsafe fn write_port_u32(self: Pin<&Self>, _: u32, _: u32) -> Result<(), ()> {
        Err(())
    }

    unsafe fn read_port_u8(self: Pin<&Self>, _: u32) -> Result<u8, ()> {
        Err(())
    }

    unsafe fn read_port_u16(self: Pin<&Self>, _: u32) -> Result<u16, ()> {
        Err(())
    }

    unsafe fn read_port_u32(self: Pin<&Self>, _: u32) -> Result<u32, ()> {
        Err(())
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
