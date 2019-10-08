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
    nametbd_syscalls_interface::block_on(async move {
        nametbd_interface_interface::register_interface(nametbd_loader_interface::ffi::INTERFACE).await.unwrap();

        loop {
            let msg = nametbd_syscalls_interface::next_interface_message().await;
            assert_eq!(msg.interface, nametbd_loader_interface::ffi::INTERFACE);
            let msg_data = nametbd_loader_interface::ffi::LoaderMessage::decode_all(&msg.actual_data).unwrap();
            let nametbd_loader_interface::ffi::LoaderMessage::Load(hash_to_load) = msg_data;
            println!("received message: {:?}", hash_to_load);
            let data = vec![];// include_bytes!("../../target/wasm32-wasi/release/preloaded.wasm");
            nametbd_syscalls_interface::emit_answer(msg.message_id.unwrap(), &nametbd_loader_interface::ffi::LoadResponse {
                result: Ok(data.to_vec())
            });
        }
    });
}
