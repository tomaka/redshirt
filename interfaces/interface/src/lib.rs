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

//! Interfaces registration.

#![deny(intra_doc_link_resolution_failure)]
#![cfg_attr(not(feature = "std"), no_std)]

use core::mem;
use parity_scale_codec::DecodeAll;

pub use ffi::InterfaceRegisterError;

pub mod ffi;

/// Registers the current program as the provider for the given interface hash.
///
/// > **Note**: Interface hashes can be found in the various `ffi` modules of the crates in the
/// >           `interfaces` directory, although that is subject to change.
///
/// Returns an error if there was already a program registered for that interface.
#[cfg(target_arch = "wasm32")] // TODO: bad
#[cfg(feature = "std")]
pub async fn register_interface(hash: [u8; 32]) -> Result<(), InterfaceRegisterError> {
    let msg = ffi::InterfaceMessage::Register(hash);
    // TODO: we unwrap cause there's always something that handles interface registration; is that correct?
    let rep: ffi::InterfaceRegisterResponse = unsafe {
        nametbd_syscalls_interface::emit_message_with_response(ffi::INTERFACE, msg)
            .await
            .unwrap()
    };
    rep.result
}

/// Registers the current program as the provider for the given interface hash.
///
/// > **Note**: Interface hashes can be found in the various `ffi` modules of the crates in the
/// >           `interfaces` directory, although that is subject to change.
///
/// Returns an error if there was already a program registered for that interface.
#[cfg(not(target_arch = "wasm32"))]
#[cfg(feature = "std")]
pub async fn register_interface(hash: [u8; 32]) -> Result<(), InterfaceRegisterError> {
    unimplemented!()
}
