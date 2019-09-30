// Copyright(c) 2019 Pierre Krieger

//! Threads.

#![deny(intra_doc_link_resolution_failure)]

use parity_scale_codec::DecodeAll;

// TODO: everything here is a draft

use std::mem;

pub mod ffi;

#[cfg(target_arch = "wasm32")] // TODO: bad
pub fn register_interface(hash: [u8; 32]) -> Result<(), ()> {
    unsafe {
        // TODO: non blocking
        let msg = ffi::InterfaceMessage::Register(hash);
        let msg_id = syscalls::emit_message(&ffi::INTERFACE, &msg, true)
            .unwrap()
            .unwrap();
        let msg = syscalls::next_message(&mut [msg_id], true).unwrap();
        match msg {
            // TODO: code style: improve syscall's API
            syscalls::Message::Response(syscalls::ffi::ResponseMessage {
                message_id,
                actual_data,
            }) => {
                assert_eq!(message_id, msg_id);
                let msg: ffi::InterfaceRegisterResponse =
                    DecodeAll::decode_all(&actual_data).unwrap();
                msg.result
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn register_interface(hash: &[u8; 32]) -> Result<(), ()> {
    unimplemented!()
}
