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
    net::{IpAddr, Ipv6Addr, SocketAddr},
    time::Duration,
};

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main())
}

struct SocketState {
    id: u32,
    connected_message: Option<MessageId>,
    read_message: Option<MessageId>,
    write_finished_message: Option<MessageId>,
}

async fn async_main() {
    // Register the ethernet and TCP interfaces.
    redshirt_interface_interface::register_interface(eth_ffi::INTERFACE)
        .await
        .unwrap();
    redshirt_interface_interface::register_interface(tcp_ffi::INTERFACE)
        .await
        .unwrap();

    let mut network = NetworkManager::<_, VecDeque<MessageId>, SocketState>::new();
    let mut sockets = HashMap::with_capacity_and_hasher(0, fnv::FnvBuildHasher::default());
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
                NetworkManagerEvent::EthernetCableOut(id, msg_id, buffer),
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
            future::Either::Right((
                NetworkManagerEvent::TcpConnected {
                    mut socket,
                    local_endpoint,
                    remote_endpoint,
                },
                _,
            )) => {
                let state = socket.user_data_mut();
                let message_id = state.connected_message.take().unwrap();
                redshirt_syscalls::emit_answer(
                    message_id,
                    &tcp_ffi::TcpOpenResponse {
                        result: Ok(tcp_ffi::TcpSocketOpen {
                            socket_id: state.id,
                            local_ip: match local_endpoint.ip() {
                                IpAddr::V4(ip) => ip.to_ipv6_mapped().segments(),
                                IpAddr::V6(ip) => ip.segments(),
                            },
                            local_port: local_endpoint.port(),
                            remote_ip: match remote_endpoint.ip() {
                                IpAddr::V4(ip) => ip.to_ipv6_mapped().segments(),
                                IpAddr::V6(ip) => ip.segments(),
                            },
                            remote_port: remote_endpoint.port(),
                        }),
                    },
                );
                continue;
            }
            future::Either::Right((NetworkManagerEvent::TcpClosed(socket), _)) => unimplemented!(),
            future::Either::Right((NetworkManagerEvent::TcpReadReady(mut socket), _)) => {
                let data = socket.read();
                assert!(!data.is_empty());
                let state = socket.user_data_mut();
                if let Some(message_id) = state.read_message.take() {
                    redshirt_syscalls::emit_answer(
                        message_id,
                        &tcp_ffi::TcpReadResponse { result: Ok(data) },
                    );
                }
                continue;
            }
            future::Either::Right((NetworkManagerEvent::TcpWriteFinished(mut socket), _)) => {
                let state = socket.user_data_mut();
                if let Some(message_id) = state.write_finished_message.take() {
                    redshirt_syscalls::emit_answer(
                        message_id,
                        &tcp_ffi::TcpWriteResponse { result: Ok(()) },
                    );
                }
                continue;
            }
        };

        if msg.interface == tcp_ffi::INTERFACE {
            let msg_data = tcp_ffi::TcpMessage::decode(msg.actual_data).unwrap();
            match msg_data {
                tcp_ffi::TcpMessage::Open(open_msg) => {
                    let new_id = next_socket_id;
                    next_socket_id += 1;

                    let inner_id = network
                        .build_tcp_socket(
                            open_msg.listen,
                            &{
                                let ip_addr = Ipv6Addr::from(open_msg.ip);
                                if let Some(ip_addr) = ip_addr.to_ipv4() {
                                    SocketAddr::new(ip_addr.into(), open_msg.port)
                                } else {
                                    SocketAddr::new(ip_addr.into(), open_msg.port)
                                }
                            },
                            SocketState {
                                id: new_id,
                                // TODO: don't unwrap
                                connected_message: Some(msg.message_id.unwrap()),
                                read_message: None,
                                write_finished_message: None,
                            },
                        )
                        .id();

                    sockets.insert(new_id, inner_id);
                }
                tcp_ffi::TcpMessage::Close(msg) => {
                    /*if let Some(inner_id) = sockets.remove(&msg.socket_id) {
                        network.tcp_socket_by_id(&inner_id).unwrap().close();
                    }*/
                }
                tcp_ffi::TcpMessage::Read(read) => {
                    // TODO: don't unwrap
                    let inner_socket_id = sockets.get_mut(&read.socket_id).unwrap();
                    let mut inner_socket = network.tcp_socket_by_id(inner_socket_id).unwrap();
                    // TODO: handle errors
                    let available = inner_socket.read();
                    if !available.is_empty() {
                        redshirt_syscalls::emit_answer(
                            msg.message_id.unwrap(),
                            &tcp_ffi::TcpReadResponse {
                                result: Ok(available),
                            },
                        );
                    } else {
                        inner_socket.user_data_mut().read_message = msg.message_id;
                    }
                }
                tcp_ffi::TcpMessage::Write(write) => {
                    // TODO: don't unwrap
                    let inner_socket_id = sockets.get_mut(&write.socket_id).unwrap();
                    let mut inner_socket = network.tcp_socket_by_id(inner_socket_id).unwrap();
                    // TODO: handle errors
                    let _ = inner_socket.set_write_buffer(write.data);
                    inner_socket.user_data_mut().write_finished_message = msg.message_id;
                }
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
