// Copyright(c) 2019 Pierre Krieger

//! Bindings for interfacing with the environment of the "kernel".

#![deny(intra_doc_link_resolution_failure)]

use parity_scale_codec::Encode;

pub mod ffi;

#[cfg(target_os = "unknown")]      // TODO: bad
pub fn register_interface(hash: &[u8; 32]) -> Result<(), ()> {
    unsafe {
        let ret = ffi::register_interface(hash as *const [u8; 32] as *const _);
        if ret == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}

#[cfg(target_os = "unknown")]      // TODO: bad
pub fn next_message(to_poll: &mut [u64], out: &mut [u8], block: bool) -> usize {
    unsafe {
        let ret = ffi::next_message(
            to_poll.as_mut_ptr(),
            to_poll.len() as u32,
            out.as_mut_ptr(),
            out.len() as u32,
            block
        );
        ret as usize
    }
}

pub fn emit_message(interface_hash: &[u8; 32], msg: &impl Encode, needs_answer: bool) -> u64 {
    unsafe {
        let buf = msg.encode();
        ffi::emit_message(interface_hash as *const [u8; 32] as *const _, buf.as_ptr(), buf.len() as u32, needs_answer)
    }
}

pub fn emit_answer(message_id: u64, msg: &impl Encode) {
    unsafe {
        let buf = msg.encode();
        let ret = ffi::emit_answer(message_id, buf.as_ptr(), buf.len() as u32);
        // TODO: ret value
    }
}
