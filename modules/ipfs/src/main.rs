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
use ipfs::{Network, NetworkEvent};
use parity_scale_codec::DecodeAll;

fn main() {
    nametbd_syscalls_interface::block_on(async_main())
}

async fn async_main() {
    nametbd_interface_interface::register_interface(nametbd_loader_interface::ffi::INTERFACE)
        .await
        .unwrap();

    let mut network = Network::start();

    loop {
        let next_interface = nametbd_syscalls_interface::next_interface_message();
        let next_net_event = Box::pin(network.next_event());
        let msg = match future::select(next_interface, next_net_event).await {
            future::Either::Left((msg, _)) => msg,
            future::Either::Right((NetworkEvent::FetchSuccess { data, user_data }, _)) => {
                let rp = nametbd_loader_interface::ffi::LoadResponse { result: Ok(data) };
                nametbd_syscalls_interface::emit_answer(user_data, &rp);
                continue;
            }
            future::Either::Right((NetworkEvent::FetchFail { user_data }, _)) => {
                let rp = nametbd_loader_interface::ffi::LoadResponse { result: Err(()) };
                nametbd_syscalls_interface::emit_answer(user_data, &rp);
                continue;
            }
        };

        assert_eq!(msg.interface, nametbd_loader_interface::ffi::INTERFACE);
        let msg_data =
            nametbd_loader_interface::ffi::LoaderMessage::decode_all(&msg.actual_data).unwrap();
        let nametbd_loader_interface::ffi::LoaderMessage::Load(hash_to_load) = msg_data;
        network.start_fetch(&hash_to_load, msg.message_id.unwrap());
    }
}
