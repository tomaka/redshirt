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

//! Lazy-loading WASM modules.

#![deny(intra_doc_link_resolution_failure)]

use parity_scale_codec::DecodeAll;
use std::mem;

pub mod ffi;

/// Tries to load a WASM module based on its hash.
///
/// Returns either the binary content of the module, or an error if no module with that hash
/// could be found.
#[cfg(target_arch = "wasm32")] // TODO: bad
pub async fn load(hash: [u8; 32]) -> Result<Vec<u8>, ()> {
    let msg = ffi::LoaderMessage::Load(hash);
    let rep: ffi::LoadResponse =
        nametbd_syscalls_interface::emit_message_with_response(ffi::INTERFACE, msg).await?;
    rep.result
}

/// Tries to load a WASM module based on its hash.
///
/// Returns either the binary content of the module, or an error if no module with that hash
/// could be found.
#[cfg(not(target_arch = "wasm32"))]
pub async fn load(hash: &[u8; 32]) -> Result<Vec<u8>, ()> {
    Err(())
}
