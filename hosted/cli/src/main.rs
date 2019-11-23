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

use futures::{channel::mpsc, pin_mut, prelude::*};
use parity_scale_codec::{DecodeAll, Encode as _};
use std::sync::Arc;

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
            .with_interface_handler(nametbd_time_interface::ffi::INTERFACE)
            .with_interface_handler(nametbd_tcp_interface::ffi::INTERFACE)
            .with_startup_process(module)
            .with_main_program([0; 32]) // TODO: just a test
            .build();

    let time = Arc::new(nametbd_time_hosted::TimerHandler::new());
    let tcp = Arc::new(nametbd_tcp_hosted::TcpState::new());

    let mut to_answer_rx = {
        let (mut to_answer_tx, to_answer_rx) = mpsc::channel(16);
        let tcp = tcp.clone();
        let time = time.clone();
        async_std::task::spawn(async move {
            loop {
                let tcp = tcp.next_event();
                let time = time.next_answer();
                pin_mut!(tcp);
                pin_mut!(time);
                let to_send = match future::select(tcp, time).await {
                    future::Either::Left((
                        nametbd_tcp_hosted::TcpResponse::Open(msg_id, msg),
                        _,
                    )) => (msg_id, msg.encode()),
                    future::Either::Left((
                        nametbd_tcp_hosted::TcpResponse::Read(msg_id, msg),
                        _,
                    )) => (msg_id, msg.encode()),
                    future::Either::Left((
                        nametbd_tcp_hosted::TcpResponse::Write(msg_id, msg),
                        _,
                    )) => (msg_id, msg.encode()),
                    future::Either::Right(((msg_id, bytes), _)) => (msg_id, bytes),
                };
                if to_answer_tx.send(to_send).await.is_err() {
                    break;
                }
            }
        });
        to_answer_rx
    };

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
                    if let Some(answer) = time.time_message(message_id, &message) {
                        system.answer_message(message_id.unwrap(), &answer);
                    }
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

            let (msg_to_respond, response_bytes) = if only_poll {
                match to_answer_rx.next().now_or_never() {
                    Some(e) => e,
                    None => continue,
                }
            } else {
                to_answer_rx.next().await
            }
            .unwrap();

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
