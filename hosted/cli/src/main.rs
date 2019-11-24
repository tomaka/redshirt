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
        &include_bytes!("../../../modules/target/wasm32-wasi/release/ipfs.wasm")[..],
    )
    .unwrap();

    let mut system =
        nametbd_wasi_hosted::register_extrinsics(nametbd_core::system::SystemBuilder::new())
            .with_interface_handler(nametbd_tcp_interface::ffi::INTERFACE)
            .with_startup_process(module)
            .with_main_program([0; 32]) // TODO: just a test
            .build();

    let tcp = nametbd_tcp_hosted::TcpState::new();

    loop {
        let result = loop {
            let only_poll = match system.run() {
                nametbd_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                    pid,
                    thread_id,
                    extrinsic,
                    params,
                } => {
                    nametbd_wasi_hosted::handle_wasi(
                        &mut system,
                        extrinsic,
                        pid,
                        thread_id,
                        params,
                    );
                    true
                }
                nametbd_core::system::SystemRunOutcome::InterfaceMessage {
                    message_id,
                    interface,
                    message,
                } if interface == nametbd_time_interface::ffi::INTERFACE => {
                    let answer = nametbd_time_hosted::time_message(&message);
                    system.answer_message(message_id.unwrap(), &answer);
                    continue;
                }
                nametbd_core::system::SystemRunOutcome::InterfaceMessage {
                    message_id,
                    interface,
                    message,
                } if interface == nametbd_tcp_interface::ffi::INTERFACE => {
                    let message: nametbd_tcp_interface::ffi::TcpMessage =
                        DecodeAll::decode_all(&message).unwrap();
                    tcp.handle_message(message_id, message).await;
                    continue;
                }
                nametbd_core::system::SystemRunOutcome::Idle => false,
                other => break other,
            };

            let event = if only_poll {
                match tcp.next_event().now_or_never() {
                    Some(e) => e,
                    None => continue,
                }
            } else {
                tcp.next_event().await
            };

            let (msg_to_respond, response_bytes) = match event {
                nametbd_tcp_hosted::TcpResponse::Accept(msg_id, msg) => (msg_id, msg.encode()),
                nametbd_tcp_hosted::TcpResponse::Listen(msg_id, msg) => (msg_id, msg.encode()),
                nametbd_tcp_hosted::TcpResponse::Open(msg_id, msg) => (msg_id, msg.encode()),
                nametbd_tcp_hosted::TcpResponse::Read(msg_id, msg) => (msg_id, msg.encode()),
                nametbd_tcp_hosted::TcpResponse::Write(msg_id, msg) => (msg_id, msg.encode()),
            };
            system.answer_message(msg_to_respond, &response_bytes);
        };

        match result {
            nametbd_core::system::SystemRunOutcome::ProgramFinished { pid, outcome } => {
                println!("Program finished {:?} => {:?}", pid, outcome);
            }
            _ => panic!(),
        }
    }
}
