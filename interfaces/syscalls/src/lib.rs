// Copyright(c) 2019 Pierre Krieger

//! Bindings for interfacing with the environment of the "kernel".

#![deny(intra_doc_link_resolution_failure)]

use parity_scale_codec::{DecodeAll, Encode};

pub use ffi::Message;

pub mod ffi;

#[cfg(target_arch = "wasm32")] // TODO: bad
pub fn next_message(to_poll: &mut [u64], block: bool) -> Option<Message> {
    unsafe {
        let mut out = Vec::with_capacity(32);
        loop {
            let ret = ffi::next_message(
                to_poll.as_mut_ptr(),
                to_poll.len() as u32,
                out.as_mut_ptr(),
                out.capacity() as u32,
                block,
            ) as usize;
            if ret == 0 {
                return None;
            }
            if ret > out.capacity() {
                out.reserve(ret);
                continue;
            }
            out.set_len(ret);
            let msg: Message = DecodeAll::decode_all(&out).unwrap();
            return Some(msg);
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub fn next_message(to_poll: &mut [u64], block: bool) -> Option<Message> {
    unimplemented!()
}

pub fn emit_message(
    interface_hash: &[u8; 32],
    msg: &impl Encode,
    needs_answer: bool,
) -> Result<Option<u64>, ()> {
    unsafe {
        let buf = msg.encode();
        let mut event_id_out = 0;
        let ret = ffi::emit_message(
            interface_hash as *const [u8; 32] as *const _,
            buf.as_ptr(),
            buf.len() as u32,
            needs_answer,
            &mut event_id_out as *mut _,
        );
        if ret != 0 {
            return Err(());
        }

        if needs_answer {
            Ok(Some(event_id_out))
        } else {
            Ok(None)
        }
    }
}

pub fn emit_answer(message_id: u64, msg: &impl Encode) -> Result<(), ()> {
    unsafe {
        let buf = msg.encode();
        let ret = ffi::emit_answer(message_id, buf.as_ptr(), buf.len() as u32);
        if ret == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}
