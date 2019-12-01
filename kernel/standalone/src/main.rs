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
#![feature(asm)]
#![feature(core_intrinsics)]
#![feature(panic_info_message)] // TODO: https://github.com/rust-lang/rust/issues/66745
#![feature(alloc_error_handler)] // TODO: https://github.com/rust-lang/rust/issues/66741

extern crate alloc;
extern crate compiler_builtins;

mod arch;

use alloc::{format, string::String};
use core::fmt::Write;
use parity_scale_codec::DecodeAll;

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

#[alloc_error_handler]
fn alloc_error_handler(_: core::alloc::Layout) -> ! {
    panic!()
}

#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    // Because the diagnostic code below might panic again, we first print a `Panic` message on
    // the top left of the screen.
    let vga_buffer = 0xb8000 as *mut u8;
    for (i, &byte) in b"Panic".iter().enumerate() {
        unsafe {
            *vga_buffer.offset(i as isize * 2) = byte;
            *vga_buffer.offset(i as isize * 2 + 1) = 0xc;
        }
    }

    let mut console = unsafe { nametbd_x86_stdout::Console::init() };

    if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
        let _ = writeln!(console, "panic occurred: {:?}", s);
    } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
        let _ = writeln!(console, "panic occurred: {:?}", s);
    } else if let Some(message) = panic_info.message() {
        let _ = Write::write_fmt(&mut console, *message);
        let _ = writeln!(console, "");
    } else {
        let _ = writeln!(console, "panic occurred");
    }

    if let Some(location) = panic_info.location() {
        let _ = writeln!(
            console,
            "panic occurred in file '{}' at line {}",
            location.file(),
            location.line()
        );
    } else {
        let _ = writeln!(
            console,
            "panic occurred but can't get location information..."
        );
    }

    loop {
        //unsafe { x86::halt() }
    }
}

// Note: don't get fooled, this is not the "official" main function.
// We have a `#![no_main]` attribute applied to this crate, meaning that this `main` function here
// is just a regular function that is called by our bootstrapping process.
fn main() -> ! {
    unsafe {
        // TODO: don't have the HEAP here, but adjust it to the available RAM
        static mut HEAP: [u8; 0x10000000] = [0; 0x10000000];
        ALLOCATOR
            .lock()
            .init(HEAP.as_mut_ptr() as usize, HEAP.len());
    }

    let mut console = unsafe { nametbd_x86_stdout::Console::init() };

    let module = nametbd_core::module::Module::from_bytes(
        &include_bytes!("../../../modules/target/wasm32-unknown-unknown/release/hello-world.wasm")
            [..],
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
                loop {
                    //unsafe { x86::halt() }
                }
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
