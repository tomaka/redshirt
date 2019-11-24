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

// TODO: enable `#![no_std]` when possible: https://github.com/rust-lang/rust/issues/56974
//#![no_std]

use parity_scale_codec::DecodeAll;

fn main() {
    futures::executor::block_on(async_main());
}

async fn async_main() -> ! {
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
                println!("Program finished {:?} => {:?}", pid, outcome);
            }
            nametbd_core::system::SystemRunOutcome::InterfaceMessage {
                interface,
                message,
                ..
            } if interface == nametbd_stdout_interface::ffi::INTERFACE => {
                let msg = nametbd_stdout_interface::ffi::StdoutMessage::decode_all(&message);
                let nametbd_stdout_interface::ffi::StdoutMessage::Message(msg) = msg.unwrap();
                console.write(&msg);
            }
            _ => panic!(),
        }
    }
}
