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
    ldr sp, =stack+0x40000
    b after_boot
"#
        );
    }
}

#[no_mangle]
extern "C" fn after_boot() -> ! {
    write_serial(b"hello world\n".iter().cloned());
    unsafe {
        asm!("b .");
        core::intrinsics::unreachable()
    }
    //crate::main()
}

fn write_uart(data: impl Iterator<Item = u8>) {
    unsafe {
        let base_ptr = 0x101f1000 as *mut u32;
        for byte in data {
            base_ptr.write_volatile(u32::from(byte));
        }
    }
}

fn write_serial(data: impl Iterator<Item = u8>) {
    unsafe {
        let base_ptr = 0x16000000 as *mut u8;
        let register = 0x16000018 as *mut u8;

        for byte in data {
            while register.read_volatile() & (1 << 5) != 0 {}
            base_ptr.write_volatile(byte);
        }
    }
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
        asm!("b .");
        core::intrinsics::unreachable()
    }
}
