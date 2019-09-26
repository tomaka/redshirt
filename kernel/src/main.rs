// Copyright(c) 2019 Pierre Krieger

#![feature(never_type)]
#![deny(intra_doc_link_resolution_failure)]

use byteorder::{ByteOrder as _, LittleEndian};
use parity_scale_codec::{DecodeAll, Encode as _};
use std::io::Write as _;

mod tcp_interface;
mod wasi;

fn main() {
    let module = kernel_core::module::Module::from_bytes(
        &include_bytes!("../../target/wasm32-wasi/release/ipfs.wasm")[..],
    );

    let mut system = wasi::register_extrinsics(kernel_core::system::System::new())
        .with_interface_handler([
            // TCP
            0x10, 0x19, 0x16, 0x2a, 0x2b, 0x0c, 0x41, 0x36, 0x4a, 0x20, 0x01, 0x51, 0x47, 0x38,
            0x27, 0x08, 0x4a, 0x3c, 0x1e, 0x07, 0x18, 0x1c, 0x27, 0x11, 0x55, 0x15, 0x1d, 0x5f,
            0x22, 0x5b, 0x16, 0x20,
        ])
        .with_main_program(module)
        .build();

    let mut tcp = tcp_interface::TcpState::new();

    loop {
        let result = futures::executor::block_on(async {
            loop {
                match system.run() {
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic,
                        params,
                    } => {
                        wasi::handle_wasi(&mut system, extrinsic, pid, thread_id, params);
                        continue;
                    }
                    kernel_core::system::SystemRunOutcome::InterfaceMessage {
                        event_id,
                        interface,
                        message,
                    } => {
                        // TODO: we assume it's TCP
                        let message: tcp::ffi::TcpMessage =
                            DecodeAll::decode_all(&message).unwrap();
                        tcp.handle_message(event_id, message);
                        continue;
                    }
                    kernel_core::system::SystemRunOutcome::Idle => {}
                    other => break other,
                }

                let (msg_to_respond, response_bytes) = match tcp.next_event().await {
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
