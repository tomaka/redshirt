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
use parity_scale_codec::DecodeAll;
use std::sync::Arc;

fn main() {
    futures::executor::block_on(async_main());
}

async fn async_main() {
    let module = redshirt_core::module::Module::from_bytes(
        &include_bytes!("../../../modules/target/wasm32-wasi/release/http-server.wasm")[..],
    )
    .unwrap();

    let mut system =
        redshirt_wasi_hosted::register_extrinsics(redshirt_core::system::SystemBuilder::new())
            .with_interface_handler(redshirt_stdout_interface::ffi::INTERFACE)
            .with_interface_handler(redshirt_time_interface::ffi::INTERFACE)
            .with_interface_handler(redshirt_tcp_interface::ffi::INTERFACE)
            .with_startup_process(module)
            .with_main_program([0; 32]) // TODO: just a test
            .build();

    let time = Arc::new(redshirt_time_hosted::TimerHandler::new());
    let tcp = Arc::new(redshirt_tcp_hosted::TcpState::new());
    let mut wasi = redshirt_wasi_hosted::WasiStateMachine::new();

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
                    future::Either::Left(((msg_id, bytes), _)) => (msg_id, bytes),
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
                redshirt_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                    pid,
                    thread_id,
                    extrinsic,
                    params,
                } => {
                    let out =
                        wasi.handle_extrinsic_call(&mut system, extrinsic, pid, thread_id, params);
                    if let redshirt_wasi_hosted::HandleOut::EmitMessage {
                        id,
                        interface,
                        message,
                    } = out
                    {
                        if interface == redshirt_stdout_interface::ffi::INTERFACE {
                            let msg =
                                redshirt_stdout_interface::ffi::StdoutMessage::decode_all(&message);
                            let redshirt_stdout_interface::ffi::StdoutMessage::Message(msg) =
                                msg.unwrap();
                            print!("{}", msg);
                        } else if interface == redshirt_time_interface::ffi::INTERFACE {
                            if let Some(answer) =
                                time.time_message(id.map(MessageId::Wasi), &message)
                            {
                                unimplemented!()
                            }
                        } else if interface == redshirt_tcp_interface::ffi::INTERFACE {
                            let message: redshirt_tcp_interface::ffi::TcpMessage =
                                DecodeAll::decode_all(&message).unwrap();
                            tcp.handle_message(id.map(MessageId::Wasi), message).await;
                        } else {
                            panic!()
                        }
                    }
                    true
                }
                redshirt_core::system::SystemRunOutcome::InterfaceMessage {
                    interface,
                    message,
                    ..
                } if interface == redshirt_stdout_interface::ffi::INTERFACE => {
                    let msg = redshirt_stdout_interface::ffi::StdoutMessage::decode_all(&message);
                    let redshirt_stdout_interface::ffi::StdoutMessage::Message(msg) = msg.unwrap();
                    print!("{}", msg);
                    continue;
                }
                redshirt_core::system::SystemRunOutcome::InterfaceMessage {
                    message_id,
                    interface,
                    message,
                    ..
                } if interface == redshirt_time_interface::ffi::INTERFACE => {
                    if let Some(answer) =
                        time.time_message(message_id.map(MessageId::Core), &message)
                    {
                        let answer = match &answer {
                            Ok(v) => Ok(&v[..]),
                            Err(()) => Err(()),
                        };
                        system.answer_message(message_id.unwrap(), answer);
                    }
                    continue;
                }
                redshirt_core::system::SystemRunOutcome::InterfaceMessage {
                    message_id,
                    interface,
                    message,
                    ..
                } if interface == redshirt_tcp_interface::ffi::INTERFACE => {
                    let message: redshirt_tcp_interface::ffi::TcpMessage =
                        DecodeAll::decode_all(&message).unwrap();
                    tcp.handle_message(message_id.map(MessageId::Core), message)
                        .await;
                    continue;
                }
                redshirt_core::system::SystemRunOutcome::Idle => false,
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

            match msg_to_respond {
                MessageId::Core(msg_id) => system.answer_message(msg_id, Ok(&response_bytes)),
                MessageId::Wasi(msg_id) => unimplemented!(),
            }
        };

        match result {
            redshirt_core::system::SystemRunOutcome::ProgramFinished { pid, outcome } => {
                println!("Program finished {:?} => {:?}", pid, outcome);
            }
            _ => panic!(),
        }
    }
}

enum MessageId {
    Core(u64),
    Wasi(redshirt_wasi_hosted::WasiMessageId),
}
