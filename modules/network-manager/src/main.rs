// Copyright (C) 2019-2020  Pierre Krieger
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

use fnv::FnvBuildHasher;
use futures::prelude::*;
use hashbrown::HashMap;
use network_manager::{NetworkManager, NetworkManagerEvent};
use redshirt_ethernet_interface::ffi as eth_ffi;
use redshirt_syscalls::ffi::DecodedInterfaceOrDestroyed;
use redshirt_syscalls::{Decode as _, MessageId};
use redshirt_tcp_interface::ffi as tcp_ffi;
use std::{
    collections::VecDeque,
    mem,
    net::{Ipv6Addr, SocketAddr},
    time::Duration,
};

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main())
}

async fn async_main() {
    // Register the ethernet and TCP interfaces.
    redshirt_interface_interface::register_interface(eth_ffi::INTERFACE)
        .await
        .unwrap();
    redshirt_interface_interface::register_interface(tcp_ffi::INTERFACE)
        .await
        .unwrap();

    let mut network = NetworkManager::<_, VecDeque<MessageId>>::new();
    let mut sockets = HashMap::<_, _, FnvBuildHasher>::default();
    let mut next_socket_id = 0u32;

    loop {
        let next_interface = redshirt_syscalls::next_interface_message();
        let next_net_event = Box::pin(network.next_event());
        let msg = match future::select(next_interface, next_net_event).await {
            future::Either::Left((DecodedInterfaceOrDestroyed::Interface(msg), _)) => msg,
            future::Either::Left((DecodedInterfaceOrDestroyed::ProcessDestroyed(_), _)) => {
                continue;
                // TODO: unimplemented
            }
            future::Either::Right((
                NetworkManagerEvent::EthernetCableOut(id, msg_id, mut buffer),
                _,
            )) => {
                debug_assert!(!buffer.is_empty());
                if let Some(msg_id) = msg_id.pop_front() {
                    redshirt_syscalls::emit_answer(msg_id, &buffer);
                } else {
                    // TODO: network driver is overloaded; we should have some backpressure system
                    todo!()
                }
                continue;
            }
            future::Either::Right((NetworkManagerEvent::TcpConnected(socket), _)) => {
                unimplemented!()
            }
            future::Either::Right((NetworkManagerEvent::TcpClosed(socket), _)) => unimplemented!(),
            future::Either::Right((NetworkManagerEvent::TcpReadReady(socket), _)) => {
                unimplemented!()
            }
            future::Either::Right((NetworkManagerEvent::TcpWriteFinished(socket), _)) => {
                unimplemented!()
            }
        };

        if msg.interface == tcp_ffi::INTERFACE {
            let msg_data = tcp_ffi::TcpMessage::decode(msg.actual_data).unwrap();
            match msg_data {
                tcp_ffi::TcpMessage::Open(open_msg) => {
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
                    /*let rp = tcp_ffi::TcpOpenResponse {
                        result
                    };
                    if let Some(message_id) = msg.message_id {
                        redshirt_syscalls::emit_answer(message_id, &rp);
                    }*/
                }
                tcp_ffi::TcpMessage::Close(msg) => {
                    /*if let Some(inner_id) = sockets.remove(&msg.socket_id) {
                        network.tcp_socket_by_id(&inner_id).unwrap().close();
                    }*/
                }
                tcp_ffi::TcpMessage::Read(_) => unimplemented!(),
                tcp_ffi::TcpMessage::Write(_) => unimplemented!(),
            }
        } else if msg.interface == eth_ffi::INTERFACE {
            let msg_data = eth_ffi::NetworkMessage::decode(msg.actual_data).unwrap();
            //log::debug!("message: {:?}", msg_data);

            match msg_data {
                eth_ffi::NetworkMessage::RegisterInterface { id, mac_address } => {
                    network
                        .register_interface((msg.emitter_pid, id), mac_address, VecDeque::new())
                        .await;
                }
                eth_ffi::NetworkMessage::UnregisterInterface(id) => {
                    network.unregister_interface(&(msg.emitter_pid, id));
                }
                eth_ffi::NetworkMessage::InterfaceOnData(id, buf) => {
                    // TODO: back-pressure here as well?
                    network.inject_interface_data(&(msg.emitter_pid, id), buf);
                    if let Some(message_id) = msg.message_id {
                        redshirt_syscalls::emit_answer(message_id, &());
                    }
                }
                eth_ffi::NetworkMessage::InterfaceWaitData(id) => {
                    let data = network.read_ethernet_cable_out(&(msg.emitter_pid, id));
                    if !data.is_empty() {
                        // TODO: don't unwrap message_id
                        redshirt_syscalls::emit_answer(msg.message_id.unwrap(), &data);
                    } else {
                        // TODO: don't unwrap message_id
                        network
                            .interface_user_data(&(msg.emitter_pid, id))
                            .push_back(msg.message_id.unwrap());
                    }
                }
            }
        } else {
            unreachable!()
        }
    }
}
