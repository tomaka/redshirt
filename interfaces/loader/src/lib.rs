// Copyright(c) 2019 Pierre Krieger

//! Threads.

#![deny(intra_doc_link_resolution_failure)]

use parity_scale_codec::DecodeAll;

// TODO: everything here is a draft

use std::mem;

pub mod ffi;

#[cfg(target_arch = "wasm32")] // TODO: bad
pub async fn load(hash: [u8; 32]) -> Result<Vec<u8>, ()> {
    let msg = ffi::LoaderMessage::Load(hash);
    let rep: ffi::LoadResponse =
        syscalls::emit_message_with_response(ffi::INTERFACE, msg).await?;
    rep.result
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn load(hash: &[u8; 32]) -> Result<Vec<u8>, ()> {
    Err(())
}
