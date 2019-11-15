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

#![deny(intra_doc_link_resolution_failure)]

use futures::prelude::*;
use parity_scale_codec::{DecodeAll, Encode as _};

fn main() {
    futures::executor::block_on(async_main());
}

async fn async_main() {
    let module = nametbd_core::module::Module::from_bytes(
        &include_bytes!("../../modules/target/wasm32-wasi/release/ipfs.wasm")[..],
    );

    let mut system = nametbd_core::system::SystemBuilder::<()>::new() // TODO: `!` instead
        .with_startup_process(module)
        .with_main_program([0; 32]) // TODO: just a test
        .build();

    loop {
        match system.run() {
            nametbd_core::system::SystemRunOutcome::Idle => {} // TODO: ? halt?
            nametbd_core::system::SystemRunOutcome::ProgramFinished { pid, outcome } => {
                println!("Program finished {:?} => {:?}", pid, outcome);
            }
            _ => panic!(),
        }
    }
}
