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

//! This program is meant to be invoked in a non-hosted environment. It never finishes.

#![no_std]
#![no_main]
#![feature(alloc_error_handler)] // TODO: https://github.com/rust-lang/rust/issues/66741

extern crate alloc;
extern crate compiler_builtins;

use alloc::format;
use parity_scale_codec::DecodeAll;

#[link(name = "entry")]
extern {}

#[global_allocator]
static ALLOCATOR: slab_allocator::LockedHeap = slab_allocator::LockedHeap::empty();

#[alloc_error_handler]
fn alloc_error_handler(_: core::alloc::Layout) -> ! {
    panic!()
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // TODO:
    let vga_buffer = 0xb8000 as *mut u8;
    for (i, &byte) in b"Panic".iter().enumerate() {
        unsafe {
            *vga_buffer.offset(i as isize * 2) = byte;
            *vga_buffer.offset(i as isize * 2 + 1) = 0xc;
        }
    }
    loop {}
}

// TODO: figure out how to remove these
#[no_mangle]
pub extern "C" fn fmod(a: f64, b: f64) -> f64 {
    libm::fmod(a, b)
}
#[no_mangle]
pub extern "C" fn fmodf(a: f32, b: f32) -> f32 {
    libm::fmodf(a, b)
}
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
pub extern "C" fn __truncdfsf2(a: f64) -> f32 {
    libm::trunc(a) as f32   // TODO: correct?
}

static mut HEAP: [u8; 65536] = [0; 65536];

#[no_mangle]
extern "C" fn loader_main() -> ! {
    let mut console = unsafe { nametbd_x86_stdout::Console::init() };
    console.write("hello world");

    loop {}

    unsafe {
        ALLOCATOR.init(HEAP.as_mut_ptr() as usize, HEAP.len()); // FIXME:
    }

    let module = nametbd_core::module::Module::from_bytes(
        &include_bytes!("../../../modules/target/wasm32-wasi/release/ipfs.wasm")[..],
    )
    .unwrap();

    let mut system = nametbd_core::system::SystemBuilder::<()>::new() // TODO: `!` instead
        .with_interface_handler(nametbd_stdout_interface::ffi::INTERFACE)
        .with_startup_process(module)
        .with_main_program([0; 32]) // TODO: just a test
        .build();

    loop {
        match system.run() {
            nametbd_core::system::SystemRunOutcome::Idle => {
                // TODO: If we don't support any interface or extrinsic, then `Idle` shouldn't
                // happen. In a normal situation, this is when we would check the status of the
                // "externalities", such as the timer.
                panic!()
            }
            nametbd_core::system::SystemRunOutcome::ProgramFinished { pid, outcome } => {
                console.write(&format!("Program finished {:?} => {:?}\n", pid, outcome));
            }
            nametbd_core::system::SystemRunOutcome::InterfaceMessage {
                interface, message, ..
            } if interface == nametbd_stdout_interface::ffi::INTERFACE => {
                let msg = nametbd_stdout_interface::ffi::StdoutMessage::decode_all(&message);
                let nametbd_stdout_interface::ffi::StdoutMessage::Message(msg) = msg.unwrap();
                console.write(&msg);
            }
            _ => panic!(),
        }
    }
}
