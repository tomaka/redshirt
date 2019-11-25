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
#![feature(alloc_error_handler)] // TODO: https://github.com/rust-lang/rust/issues/66741
#![feature(start)] // TODO: https://github.com/rust-lang/rust/issues/29633

extern crate alloc;

use alloc::format;
use parity_scale_codec::DecodeAll;

#[global_allocator]
static ALLOCATOR: slab_allocator::LockedHeap = slab_allocator::LockedHeap::empty();

#[alloc_error_handler]
fn foo(_: core::alloc::Layout) -> ! {
    panic!()
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {} // TODO:
}

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
    main()
}

// TODO: wtf is that
// http://www.dbp-consulting.com/tutorials/debugging/linuxProgramStartup.html
#[no_mangle]
fn __libc_csu_init() {}
#[no_mangle]
fn __libc_csu_fini() {}
#[no_mangle]
fn __libc_start_main() {}

fn main() -> ! {
    unsafe {
        ALLOCATOR.init(0x0, 0x8000); // FIXME:
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

    let mut console = unsafe { nametbd_x86_stdout::Console::init() };

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
