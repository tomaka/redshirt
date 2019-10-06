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

use futures::prelude::*;
use parity_scale_codec::DecodeAll;

fn main() {
    syscalls::block_on(async move {
        interface::register_interface(loader::ffi::INTERFACE).await.unwrap();

        loop {
            let msg = syscalls::next_interface_message().await;
            assert_eq!(msg.interface, loader::ffi::INTERFACE);
            let msg_data = loader::ffi::LoaderMessage::decode_all(&msg.actual_data).unwrap();
            let loader::ffi::LoaderMessage::Load(hash_to_load) = msg_data;
            println!("received message: {:?}", hash_to_load);
            let data = include_bytes!("../../target/wasm32-wasi/release/preloaded.wasm");
            syscalls::emit_answer(msg.message_id.unwrap(), &loader::ffi::LoadResponse {
                result: Ok(data.to_vec())
            });
        }
    });
}
