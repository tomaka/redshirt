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
use parity_scale_codec::DecodeAll;
use redshirt_network_interface::ffi;
use redshirt_syscalls_interface::ffi::InterfaceOrDestroyed;
use std::{
    net::{Ipv6Addr, SocketAddr},
    time::Duration,
};

fn main() {
    std::panic::set_hook(Box::new(|info| {
        redshirt_stdout_interface::stdout(format!("Panic: {}\n", info));
    }));

    redshirt_syscalls_interface::block_on(async_main())
}

async fn async_main() {
    redshirt_interface_interface::register_interface(ffi::INTERFACE)
        .await
        .unwrap();

    let mut network = NetworkManager::new();
    let mut sockets = HashMap::new();
    let mut next_socket_id = 0u32;

    loop {
        let next_interface = redshirt_syscalls_interface::next_interface_message();
        let next_net_event = Box::pin(network.next_event());
        let msg = match future::select(next_interface, next_net_event).await {
            future::Either::Left((InterfaceOrDestroyed::Interface(msg), _)) => msg,
            future::Either::Left((InterfaceOrDestroyed::ProcessDestroyed(_), _)) => {
                unimplemented!()
            }
            future::Either::Right((NetworkManagerEvent::EthernetCableOut(id, buffer), _)) => {
                redshirt_stdout_interface::stdout(format!("data out: {:?}\n", buffer));
                /*let rp = ffi::LoadResponse { result: Ok(data) };
                redshirt_syscalls_interface::emit_answer(user_data, &rp);*/
                continue;
            }
            _ => unimplemented!(), // TODO:
        };

        assert_eq!(msg.interface, ffi::INTERFACE);
        let msg_data = ffi::TcpMessage::decode_all(&msg.actual_data).unwrap();
        redshirt_stdout_interface::stdout(format!("message: {:?}\n", msg_data));

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
                    redshirt_syscalls_interface::emit_answer(message_id, &rp);
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
                network.register_interface((msg.emitter_pid, id), mac_address);
            }
            ffi::TcpMessage::UnregisterInterface(id) => {
                network.unregister_interface(&(msg.emitter_pid, id));
            }
            ffi::TcpMessage::InterfaceOnData(id, buf) => {
                network.inject_interface_data(&(msg.emitter_pid, id), buf);
                if let Some(message_id) = msg.message_id {
                    redshirt_syscalls_interface::emit_answer(message_id, &());
                }
            }
            ffi::TcpMessage::InterfaceWaitData(id) => {
                unimplemented!()
                /*network.inject_interface_data(id, buf);
                redshirt_syscalls_interface::emit_answer(msg.id, &());*/
            }
        }
    }
}
