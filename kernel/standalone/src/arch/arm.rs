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

#![cfg(any(target_arch = "arm", target_arch = "aarch64"))]

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
    "#
    );

    // Only one CPU reaches here.

    // Set up the stack.
    asm!(r#"
    .comm stack, 0x400000, 8
    ldr sp, =stack+0x400000"#:::"memory":"volatile");

    cpu_enter()
}

#[no_mangle]
fn cpu_enter() -> ! {
    crate::mem_alloc::initialize();

    let kernel = crate::kernel::Kernel::init(crate::kernel::KernelConfig {
        num_cpus: 1,
        ..Default::default()
    });

    kernel.run()
}

// TODO: figure out how to remove these
#[no_mangle]
pub extern "C" fn fmin(a: f64, b: f64) -> f64 {
    libm::fmin(a, b)
}
#[no_mangle]
pub extern "C" fn fminf(a: f32, b: f32) -> f32 {
    libm::fminf(a, b)
}
#[no_mangle]
pub extern "C" fn fmax(a: f64, b: f64) -> f64 {
    libm::fmax(a, b)
}
#[no_mangle]
pub extern "C" fn fmaxf(a: f32, b: f32) -> f32 {
    libm::fmaxf(a, b)
}
#[no_mangle]
pub extern "C" fn __aeabi_d2f(a: f64) -> f32 {
    libm::trunc(a) as f32 // TODO: correct?
}

// TODO: no_mangle and naked because it's called at initialization; attributes should eventually be removed
#[no_mangle]
#[naked]
// TODO: define the semantics of that
pub fn halt() -> ! {
    unsafe {
        loop {
            asm!(r#"wfe"#);
        }
    }
}

pub unsafe fn write_port_u8(port: u32, data: u8) {}

pub unsafe fn write_port_u16(port: u32, data: u16) {}

pub unsafe fn write_port_u32(port: u32, data: u32) {}

pub unsafe fn read_port_u8(port: u32) -> u8 {
    0
}

pub unsafe fn read_port_u16(port: u32) -> u16 {
    0
}

pub unsafe fn read_port_u32(port: u32) -> u32 {
    0
}
