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

#![recursion_limit = "2048"]

use futures::prelude::*;
use hashbrown::HashMap;
use network_manager::{NetworkManager, NetworkManagerEvent};
use redshirt_ethernet_interface::ffi as eth_ffi;
use redshirt_interface_interface::DecodedInterfaceOrDestroyed;
use redshirt_syscalls::{Decode as _, MessageId};
use redshirt_tcp_interface::ffi as tcp_ffi;
use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv6Addr, SocketAddr},
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
    let mut eth_registration = redshirt_interface_interface::register_interface(eth_ffi::INTERFACE)
        .await
        .unwrap();
    let mut tcp_registration = redshirt_interface_interface::register_interface(tcp_ffi::INTERFACE)
        .await
        .unwrap();

    let mut network = NetworkManager::<_, VecDeque<MessageId>, SocketState>::new();
    let mut sockets = HashMap::with_capacity_and_hasher(0, fnv::FnvBuildHasher::default());
    let mut next_socket_id = 0u32;

    // TODO: re-review all this code

    loop {
        futures::select! {
            interface_event = eth_registration.next_message_raw().fuse() => {
                match interface_event {
                    DecodedInterfaceOrDestroyed::Interface(msg) => {
                        let msg_data = eth_ffi::NetworkMessage::decode(msg.actual_data).unwrap();
                        match msg_data {
                            eth_ffi::NetworkMessage::RegisterInterface { id, mac_address } => {
                                network
                                    .register_interface((msg.emitter_pid, id), mac_address, VecDeque::new())
                                    .await
                                    .unwrap(); // TODO: don't unwrap
                            }
                            eth_ffi::NetworkMessage::UnregisterInterface(id) => {
                                network.interface_by_id((msg.emitter_pid, id)).unwrap().unregister();
                            }
                            eth_ffi::NetworkMessage::InterfaceOnData(id, buf) => {
                                // TODO: back-pressure here as well?
                                network.interface_by_id((msg.emitter_pid, id)).unwrap().inject_data(buf);
                                if let Some(message_id) = msg.message_id {
                                    redshirt_interface_interface::emit_answer(message_id, &());
                                }
                            }
                            eth_ffi::NetworkMessage::InterfaceWaitData(id) => {
                                let data = network
                                .interface_by_id((msg.emitter_pid, id)).unwrap().read_ethernet_cable_out();
                                if !data.is_empty() {
                                    // TODO: don't unwrap message_id
                                    redshirt_interface_interface::emit_answer(msg.message_id.unwrap(), &data);
                                } else {
                                    // TODO: don't unwrap message_id
                                    network
                                        .interface_by_id((msg.emitter_pid, id)).unwrap()
                                        .user_data()
                                        .push_back(msg.message_id.unwrap());
                                }
                            }
                        }
                    },
                    DecodedInterfaceOrDestroyed::ProcessDestroyed(_) => {
                        continue;
                        // TODO: unimplemented
                    }
                }
            }
            interface_event = tcp_registration.next_message_raw().fuse() => {
                match interface_event {
                    DecodedInterfaceOrDestroyed::Interface(msg) => {
                        // TODO: don't unwrap
                        let msg_data = tcp_ffi::TcpMessage::decode(msg.actual_data).unwrap();
                        match msg_data {
                            tcp_ffi::TcpMessage::Open(open_msg) => {
                                let message_id = match msg.message_id {
                                    Some(m) => m,
                                    None => continue,
                                };

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
                                            connected_message: Some(message_id),
                                            read_message: None,
                                            write_finished_message: None,
                                        },
                                    )
                                    .id();

                                sockets.insert(new_id, inner_id);
                            }
                            tcp_ffi::TcpMessage::Close(close) => {
                                if let Some(inner_id) = sockets.get_mut(&close.socket_id) {
                                    let mut socket = network.tcp_socket_by_id(&inner_id).unwrap();
                                    if socket.closed() {
                                        if let Some(message_id) = msg.message_id {
                                            redshirt_interface_interface::emit_answer(
                                                message_id,
                                                &tcp_ffi::TcpCloseResponse {
                                                    result: Err(tcp_ffi::TcpCloseError::ConnectionFinished),
                                                },
                                            );
                                        }
                                        continue;
                                    }

                                    if socket.close().is_ok() {
                                        if let Some(message_id) = msg.message_id {
                                            redshirt_interface_interface::emit_answer(
                                                message_id,
                                                &tcp_ffi::TcpCloseResponse { result: Ok(()) },
                                            );
                                        }
                                    } else if let Some(message_id) = msg.message_id {
                                        redshirt_interface_interface::emit_answer(
                                            message_id,
                                            &tcp_ffi::TcpCloseResponse {
                                                result: Err(tcp_ffi::TcpCloseError::FinAlreaySent),
                                            },
                                        );
                                    }
                                } else if let Some(message_id) = msg.message_id {
                                    redshirt_interface_interface::emit_answer(
                                        message_id,
                                        &tcp_ffi::TcpCloseResponse {
                                            result: Err(tcp_ffi::TcpCloseError::InvalidSocket),
                                        },
                                    );
                                }
                            }
                            tcp_ffi::TcpMessage::Read(read) => {
                                let message_id = match msg.message_id {
                                    Some(m) => m,
                                    None => continue,
                                };

                                if let Some(inner_socket_id) = sockets.get_mut(&read.socket_id) {
                                    let mut inner_socket = network.tcp_socket_by_id(inner_socket_id).unwrap();
                                    if inner_socket.closed() {
                                        redshirt_interface_interface::emit_answer(
                                            message_id,
                                            &tcp_ffi::TcpReadResponse {
                                                result: Err(tcp_ffi::TcpReadError::ConnectionFinished),
                                            },
                                        );
                                        continue;
                                    }

                                    // TODO: handle errors
                                    let available = inner_socket.read();
                                    if !available.is_empty() {
                                        redshirt_interface_interface::emit_answer(
                                            message_id,
                                            &tcp_ffi::TcpReadResponse {
                                                result: Ok(available),
                                            },
                                        );
                                    } else {
                                        inner_socket.user_data_mut().read_message = Some(message_id);
                                    }
                                } else {
                                    redshirt_interface_interface::emit_answer(
                                        message_id,
                                        &tcp_ffi::TcpReadResponse {
                                            result: Err(tcp_ffi::TcpReadError::InvalidSocket),
                                        },
                                    );
                                }
                            }
                            tcp_ffi::TcpMessage::Write(write) => {
                                if let Some(inner_socket_id) = sockets.get_mut(&write.socket_id) {
                                    let mut inner_socket = network.tcp_socket_by_id(inner_socket_id).unwrap();
                                    if inner_socket.closed() {
                                        if let Some(message_id) = msg.message_id {
                                            redshirt_interface_interface::emit_answer(
                                                message_id,
                                                &tcp_ffi::TcpWriteResponse {
                                                    result: Err(tcp_ffi::TcpWriteError::ConnectionFinished),
                                                },
                                            );
                                        }
                                        continue;
                                    }
                                    if inner_socket.close_called() {
                                        if let Some(message_id) = msg.message_id {
                                            redshirt_interface_interface::emit_answer(
                                                message_id,
                                                &tcp_ffi::TcpWriteResponse {
                                                    result: Err(tcp_ffi::TcpWriteError::FinAlreaySent),
                                                },
                                            );
                                        }
                                        continue;
                                    }

                                    // TODO: handle errors
                                    let _ = inner_socket.set_write_buffer(write.data);
                                    inner_socket.user_data_mut().write_finished_message = msg.message_id;
                                } else if let Some(message_id) = msg.message_id {
                                    redshirt_interface_interface::emit_answer(
                                        message_id,
                                        &tcp_ffi::TcpWriteResponse {
                                            result: Err(tcp_ffi::TcpWriteError::InvalidSocket),
                                        },
                                    );
                                }
                            }
                            tcp_ffi::TcpMessage::Destroy(socket_id) => {
                                if let Some(inner_id) = sockets.remove(&socket_id) {
                                    let mut socket = network.tcp_socket_by_id(&inner_id).unwrap();
                                    let local_state = socket.user_data_mut();
                                    // TODO: connected_message should be None, or the user
                                    // managed to guess an ID that hasn't been reported yet
                                    if let Some(message_id) = local_state.read_message.take() {
                                        redshirt_interface_interface::emit_answer(
                                            message_id,
                                            &tcp_ffi::TcpReadResponse {
                                                result: Err(tcp_ffi::TcpReadError::InvalidSocket),
                                            },
                                        );
                                    }
                                    if let Some(message_id) = local_state.write_finished_message.take() {
                                        redshirt_interface_interface::emit_answer(
                                            message_id,
                                            &tcp_ffi::TcpWriteResponse {
                                                result: Err(tcp_ffi::TcpWriteError::InvalidSocket),
                                            },
                                        );
                                    }
                                    socket.reset();
                                }
                            }
                        }
                    },
                    DecodedInterfaceOrDestroyed::ProcessDestroyed(_) => {
                        continue;
                        // TODO: unimplemented
                    }
                }
            }
            net_event = network.next_event().fuse() => {
                match net_event {
                    NetworkManagerEvent::EthernetCableOut(mut interface) => {
                        // There is data available for sending to the network. We only actually
                        // send data if there is a `InterfaceWaitData` message available to
                        // respond to.
                        // If that is not the case, then we don't pull any data, which also causes
                        // the interface to not emit any data, and propagates the back-pressure to
                        // the sockets. When a `InterfaceWaitData` later arrives, we try to call
                        // `read_ethernet_cable_out` again.
                        if let Some(msg_id) = interface.user_data().pop_front() {
                            let buffer = interface.read_ethernet_cable_out();
                            debug_assert!(!buffer.is_empty());
                            redshirt_interface_interface::emit_answer(msg_id, &buffer);
                        }
                    }
                    NetworkManagerEvent::TcpConnected {
                        mut socket,
                        local_endpoint,
                        remote_endpoint,
                    } => {
                        let state = socket.user_data_mut();
                        let message_id = state.connected_message.take().unwrap();
                        redshirt_interface_interface::emit_answer(
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
                    }
                    NetworkManagerEvent::TcpClosed(mut socket) => {
                        let state = socket.user_data_mut();
                        if let Some(message_id) = state.connected_message.take() {
                            redshirt_interface_interface::emit_answer(
                                message_id,
                                &tcp_ffi::TcpOpenResponse { result: Err(()) },
                            );
                        }
                        if let Some(message_id) = state.read_message.take() {
                            redshirt_interface_interface::emit_answer(
                                message_id,
                                &tcp_ffi::TcpReadResponse {
                                    result: Err(tcp_ffi::TcpReadError::ConnectionFinished),
                                },
                            );
                        }
                        if let Some(message_id) = state.write_finished_message.take() {
                            redshirt_interface_interface::emit_answer(
                                message_id,
                                &tcp_ffi::TcpWriteResponse {
                                    result: Err(tcp_ffi::TcpWriteError::ConnectionFinished),
                                },
                            );
                        }
                    }
                    NetworkManagerEvent::TcpReadReady(mut socket) => {
                        let state = socket.user_data_mut();
                        if let Some(message_id) = state.read_message.take() {
                            let data = socket.read();
                            debug_assert!(!data.is_empty());
                            redshirt_interface_interface::emit_answer(
                                message_id,
                                &tcp_ffi::TcpReadResponse { result: Ok(data) },
                            );
                        }
                    }
                    NetworkManagerEvent::TcpWriteFinished(mut socket) => {
                        let state = socket.user_data_mut();
                        if let Some(message_id) = state.write_finished_message.take() {
                            redshirt_interface_interface::emit_answer(
                                message_id,
                                &tcp_ffi::TcpWriteResponse { result: Ok(()) },
                            );
                        }
                    }
                }
            }
        }
    }
}
