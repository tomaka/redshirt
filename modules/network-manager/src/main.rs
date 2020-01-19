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
use network_manager::{NetworkManager, NetworkManagerEvent};
use redshirt_network_interface::ffi;
use redshirt_syscalls::ffi::DecodedInterfaceOrDestroyed;
use redshirt_syscalls::{Decode as _, MessageId};
use std::{
    mem,
    net::{Ipv6Addr, SocketAddr},
    time::Duration,
};

fn main() {
    std::panic::set_hook(Box::new(|info| {
        redshirt_log_interface::log(
            redshirt_log_interface::Level::Error,
            &format!("Panic: {}", info),
        );
    }));

    redshirt_syscalls::block_on(async_main())
}

async fn async_main() {
    redshirt_interface_interface::register_interface(ffi::INTERFACE)
        .await
        .unwrap();

    let mut network = NetworkManager::<_, Option<MessageId>>::new();
    let mut sockets = HashMap::new();
    let mut next_socket_id = 0u32;

    loop {
        let next_interface = redshirt_syscalls::next_interface_message();
        let next_net_event = Box::pin(network.next_event());
        let msg = match future::select(next_interface, next_net_event).await {
            future::Either::Left((DecodedInterfaceOrDestroyed::Interface(msg), _)) => msg,
            future::Either::Left((DecodedInterfaceOrDestroyed::ProcessDestroyed(_), _)) => {
                unimplemented!()
            }
            future::Either::Right((
                NetworkManagerEvent::EthernetCableOut(id, msg_id, mut buffer),
                _,
            )) => {
                if let Some(msg_id) = msg_id.take() {
                    let data = mem::replace(&mut *buffer, Vec::new());
                    debug_assert!(!data.is_empty());
                    redshirt_syscalls::emit_answer(msg_id, &data);
                }
                continue;
            }
            _ => unimplemented!(), // TODO:
        };

        assert_eq!(msg.interface, ffi::INTERFACE);
        let msg_data = ffi::TcpMessage::decode(msg.actual_data).unwrap();
        redshirt_log_interface::log(
            redshirt_log_interface::Level::Debug,
            &format!("message: {:?}", msg_data),
        );

        match msg_data {
            ffi::TcpMessage::Open(open_msg) => {
                let result = network.build_tcp_socket(open_msg.listen, &{
                    let ip_addr = Ipv6Addr::from(open_msg.ip);
                    if let Some(ip_addr) = ip_addr.to_ipv4() {
                        SocketAddr::new(ip_addr.into(), open_msg.port)
                    } else {
                        SocketAddr::new(ip_addr.into(), open_msg.port)
                    }
                });

                let result = match result {
                    socket/*Ok(socket)*/ => {
                        let new_id = next_socket_id;
                        next_socket_id += 1;
                        sockets.insert(new_id, socket.id());
                        //Ok(new_id)
                    },
                    //Err(err) => Err(err)
                };

                // TODO: do this when connected, duh
                /*let rp = ffi::TcpOpenResponse {
                    result
                };
                if let Some(message_id) = msg.message_id {
                    redshirt_syscalls::emit_answer(message_id, &rp);
                }*/
            }
            ffi::TcpMessage::Close(msg) => {
                if let Some(inner_id) = sockets.remove(&msg.socket_id) {
                    network.tcp_socket_by_id(&inner_id).unwrap().close();
                }
            }
            ffi::TcpMessage::Read(_) => unimplemented!(),
            ffi::TcpMessage::Write(_) => unimplemented!(),
            ffi::TcpMessage::RegisterInterface { id, mac_address } => {
                network.register_interface((msg.emitter_pid, id), mac_address, None::<MessageId>);
            }
            ffi::TcpMessage::UnregisterInterface(id) => {
                network.unregister_interface(&(msg.emitter_pid, id));
            }
            ffi::TcpMessage::InterfaceOnData(id, buf) => {
                network.inject_interface_data(&(msg.emitter_pid, id), buf);
                if let Some(message_id) = msg.message_id {
                    redshirt_syscalls::emit_answer(message_id, &());
                }
            }
            ffi::TcpMessage::InterfaceWaitData(id) => {
                let data = network.read_ethernet_cable_out(&(msg.emitter_pid, id));
                if !data.is_empty() {
                    // TODO: don't unwrap message_id
                    redshirt_syscalls::emit_answer(msg.message_id.unwrap(), &data);
                } else {
                    // TODO: check if already set
                    // TODO: don't unwrap message_id
                    *network.interface_user_data(&(msg.emitter_pid, id)) =
                        Some(msg.message_id.unwrap());
                }
            }
        }
    }
}
