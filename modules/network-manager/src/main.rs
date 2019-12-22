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
use hashbrown::HashMap;
use redshirt_network_interface::ffi;
use parity_scale_codec::DecodeAll;
use std::{net::SocketAddr, time::Duration};

fn main() {
    redshirt_syscalls_interface::block_on(async_main())
}

async fn async_main() {
    redshirt_interface_interface::register_interface(ffi::INTERFACE)
        .await
        .unwrap();

    let mut network = NetworkManager::new();
    let mut sockets = HashMap::new();
    let mut next_socket_id = 0u64;

    loop {
        let next_interface = redshirt_syscalls_interface::next_interface_message();
        let next_net_event = Box::pin(network.next_event());
        let msg = match future::select(next_interface, next_net_event).await {
            future::Either::Left((msg, _)) => msg,
            future::Either::Right((NetworkEvent::FetchSuccess { data, user_data }, _)) => {
                let rp = ffi::LoadResponse { result: Ok(data) };
                redshirt_syscalls_interface::emit_answer(user_data, &rp);
                continue;
            }
            future::Either::Right((NetworkEvent::FetchFail { user_data }, _)) => {
                let rp = ffi::LoadResponse { result: Err(()) };
                redshirt_syscalls_interface::emit_answer(user_data, &rp);
                continue;
            }
        };

        assert_eq!(msg.interface, ffi::INTERFACE);
        let msg_data = ffi::TcpMessage::decode_all(&msg.actual_data).unwrap();

        match msg_data {
            ffi::TcpMessage::Listen(_) => {

            },
            ffi::TcpMessage::Accept(_) => {

            },
            ffi::TcpMessage::Open(msg) => {
                let result = network.tcp_connect({
                    let ip_addr = Ipv6Addr::from(msg.ip);
                    if let Some(ip_addr) = ip_addr.to_ipv4() {
                        SocketAddr::new(ip_addr.into(), msg.port)
                    } else {
                        SocketAddr::new(ip_addr.into(), msg.port)
                    }
                });

                let result = match result {
                    Ok(id) => {
                        let new_id = next_socket_id;
                        next_socket_id += 1;
                        sockets.insert(new_id, id);
                        new_id
                    },
                    Err(err) => Err(err)
                };

                let rp = ffi::TcpOpenResponse { result };
                redshirt_syscalls_interface::emit_answer(msg.id, &rp);
            },
            ffi::TcpMessage::Close(msg) => {
                if let Some(inner_id) = sockets.remove(&msg.id) {
                    network.socket_by_id(inner_id).unwrap().close();
                }
            },
            ffi::TcpMessage::Read(_) => {

            },
            ffi::TcpMessage::Write(_) => {

            },
            ffi::TcpMessage::RegisterInterface { id, mac_address } => {
                network.register_interface((msg.emitter_pid, id), mac_address);
            },
            ffi::TcpMessage::UnregisterInterface(id) => {
                network.unregister_interface((msg.emitter_pid, id));
            },
            ffi::TcpMessage::InterfaceOnData(_, _) => {

            },
            ffi::TcpMessage::InterfaceWaitData(_) => {

            },
        }
    }
}
