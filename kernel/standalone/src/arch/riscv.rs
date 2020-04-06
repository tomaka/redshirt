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

#![cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]

use crate::arch::{PlatformSpecific, PortErr};

use alloc::sync::Arc;
use core::{
    fmt::{self, Write as _},
    iter,
    num::NonZeroU32,
    pin::Pin,
};
use futures::prelude::*;
use redshirt_kernel_log_interface::ffi::KernelLogMethod;

mod panic;

/// This is the main entry point of the kernel for RISC-V architectures.
#[no_mangle]
#[naked]
unsafe extern "C" fn _start() -> ! {
    // Disable interrupts and clear pending interrupts.
    asm!("csrw mie, 0 ; csrw mip, 0");

    // TODO: ???
    /*asm!("
    .option push
    .option norelax
        la gp, __global_pointer$
    .option pop
    ":::"memory":"volatile");*/

    // Zero the BSS segment.
    // TODO: we pray here that the compiler doesn't use the stack
    let mut ptr = __bss_start;
    while ptr < __bss_end {
        ptr.write_volatile(0);
        ptr = ptr.add(1);
    }

    // Set up the stack.
    // TODO: better way
    asm!(r#"
    .comm stack, 0x2000, 8

    la sp, stack
    lui t0, %hi(0x2000)
    add t0, t0, %lo(0x2000)
    add sp, sp, t0

    add s0, sp, zero"#:::"memory":"volatile");

    cpu_enter();
}

extern "C" {
    static mut __bss_start: *mut u8;
    static mut __bss_end: *mut u8;
}

/// Main Rust entry point.
#[no_mangle]
fn cpu_enter() -> ! {
    panic!("Hello world!");
}

// TODO: why is this symbol required?
#[no_mangle]
fn abort() -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}

/// Implementation of [`PlatformSpecific`].
struct PlatformSpecificImpl {}

impl PlatformSpecific for PlatformSpecificImpl {
    type TimerFuture = future::Pending<()>;

    fn num_cpus(self: Pin<&Self>) -> NonZeroU32 {
        NonZeroU32::new(1).unwrap()
    }

    fn monotonic_clock(self: Pin<&Self>) -> u128 {
        unimplemented!()
    }

    fn timer(self: Pin<&Self>, deadline: u128) -> Self::TimerFuture {
        unimplemented!()
    }

    fn write_log(&self, message: &str) {
        unimplemented!()
    }

    fn set_logger_method(&self, method: KernelLogMethod) {
        unimplemented!()
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
