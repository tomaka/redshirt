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

//! Loading WASM modules.
//!
//! This interface is a bit special, as it is used by the kernel in order to load WASM modules.

#![deny(intra_doc_link_resolution_failure)]
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use futures::prelude::*;

pub mod ffi;

/// Tries to load a WASM module based on its hash.
///
/// Returns either the binary content of the module, or an error if no module with that hash
/// could be found.
// TODO: better error type
pub fn load(hash: [u8; 32]) -> impl Future<Output = Result<Vec<u8>, ()>> {
    unsafe {
        let msg = ffi::LoaderMessage::Load(hash);
        match redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg) {
            Ok(fut) => fut.map(|rep: ffi::LoadResponse| rep.result).left_future(),
            Err(_) => future::ready(Err(())).right_future(),
        }
    }
}
