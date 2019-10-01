// Copyright(c) 2019 Pierre Krieger

#![feature(never_type)]
#![deny(intra_doc_link_resolution_failure)]

use byteorder::{ByteOrder as _, LittleEndian};
use futures::prelude::*;
use parity_scale_codec::{DecodeAll, Encode as _};
use std::io::Write as _;

mod tcp_interface;
mod wasi;

fn main() {
    let module = kernel_core::module::Module::from_bytes(
        &include_bytes!("../../modules/target/wasm32-wasi/release/ipfs.wasm")[..],
    );

    let mut system = wasi::register_extrinsics(kernel_core::system::System::new())
        .with_interface_handler(tcp::ffi::INTERFACE)
        .with_main_program(module)
        .build();

    let mut tcp = tcp_interface::TcpState::new();

    loop {
        let result = futures::executor::block_on(async {
            loop {
                let only_poll = match system.run() {
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic,
                        params,
                    } => {
                        wasi::handle_wasi(&mut system, extrinsic, pid, thread_id, params);
                        true
                    }
                    kernel_core::system::SystemRunOutcome::InterfaceMessage {
                        event_id,
                        interface,
                        message,
                    } if interface == tcp::ffi::INTERFACE => {
                        let message: tcp::ffi::TcpMessage =
                            DecodeAll::decode_all(&message).unwrap();
                        tcp.handle_message(event_id, message);
                        continue;
                    }
                    kernel_core::system::SystemRunOutcome::Idle => false,
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
                    tcp_interface::TcpResponse::Open(msg_id, msg) => (msg_id, msg.encode()),
                    tcp_interface::TcpResponse::Read(msg_id, msg) => (msg_id, msg.encode()),
                    tcp_interface::TcpResponse::Write(msg_id, msg) => (msg_id, msg.encode()),
                };
                system.answer_event(msg_to_respond, &response_bytes);
            }
        });

        match result {
            kernel_core::system::SystemRunOutcome::ProgramFinished { pid, return_value } => {
                println!("Program finished {:?} => {:?}", pid, return_value)
            }
            kernel_core::system::SystemRunOutcome::ProgramCrashed { pid, error } => {
                println!("Program crashed {:?} => {:?}", pid, error);
            }
            _ => panic!(),
        }
    }
}
