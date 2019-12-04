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

#[no_mangle]
extern "C" fn dummy_fn() {
    unsafe {
        asm!(
            r#"
.comm stack, 0x40000, 8

.globl _start
_start:
    // Detect which CPU we are. Halt all CPUs except the first one.
    // TODO: this is specific to the Raspi2
    mrc p15, 0, r5, c0, c0, 5
    and r5, r5, #3
    cmp r5, #0
    bne .halt

    // Only one CPU reaches here.

    // Set up the stack.
    ldr sp, =stack+0x40000
    // Jump to the Rust code.
    b after_boot

.halt:
    wfe
    b .halt
"#
        );
    }
}

#[no_mangle]
extern "C" fn after_boot() -> ! {
    init_uart();
    for byte in b"hello world\n".iter().cloned() {
        write_uart(byte);
    }

    unsafe {
        asm!("b .");
        core::intrinsics::unreachable()
    }

    /*let kernel = crate::kernel::Kernel::init(crate::kernel::KernelConfig {
        num_cpus: 1,
        ..Default::default()
    });

    kernel.run()*/
}

const GPIO_BASE: usize = 0x3F200000;
const UART0_BASE: usize = 0x3F201000;

fn init_uart() {
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
    }
}

fn write_uart(byte: u8) {
    unsafe {
        // Wait for UART to become ready to transmit.
        while (((UART0_BASE + 0x18) as *mut u32).read_volatile() & (1 << 5)) != 0 {}
        ((UART0_BASE + 0x0) as *mut u32).write_volatile(u32::from(byte));
    }
}

fn delay(count: i32) {
    // TODO: asm!("__delay_%=: subs %[count], %[count], #1; bne __delay_%=\n" : "=r"(count): [count]"0"(count) : "cc");
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

// TODO: define the semantics of that
pub fn halt() -> ! {
    unsafe {
        loop {
            asm!(r#"wfe"#);
        }

        core::intrinsics::unreachable()
    }
}
