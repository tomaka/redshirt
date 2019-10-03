// Copyright(c) 2019 Pierre Krieger

//! Interfaces registration.

#![deny(intra_doc_link_resolution_failure)]

use parity_scale_codec::DecodeAll;
use std::mem;

pub use ffi::InterfaceRegisterError;

pub mod ffi;

/// Registers the current program as the provider for the given interface hash.
///
/// > **Note**: Interface hashes can be found in the various `ffi` modules of the crates in the
/// >           `interfaces` directory, although that is subject to change.
///
/// Returns an error if there was already a program registered for that interface.
#[cfg(target_arch = "wasm32")] // TODO: bad
pub async fn register_interface(hash: [u8; 32]) -> Result<(), InterfaceRegisterError> {
    let msg = ffi::InterfaceMessage::Register(hash);
    // TODO: we unwrap cause there's always something that handles interface registration; is that correct?
    let rep: ffi::InterfaceRegisterResponse =
        syscalls::emit_message_with_response(ffi::INTERFACE, msg).await.unwrap();
    rep.result
}

/// Registers the current program as the provider for the given interface hash.
///
/// > **Note**: Interface hashes can be found in the various `ffi` modules of the crates in the
/// >           `interfaces` directory, although that is subject to change.
///
/// Returns an error if there was already a program registered for that interface.
#[cfg(not(target_arch = "wasm32"))]
pub async fn register_interface(hash: &[u8; 32]) -> Result<(), InterfaceRegisterError> {
    unimplemented!()
}
