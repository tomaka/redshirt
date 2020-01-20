// Copyright (C) 2019-2020  Pierre Krieger
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

//! Generating cryptographically-secure random data.

#![deny(intra_doc_link_resolution_failure)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use core::convert::TryFrom;

pub mod ffi;

/// Generate `len` bytes of random data and returns them.
#[cfg(feature = "std")]
pub async fn generate(len: usize) -> Vec<u8> {
    unsafe {
        let mut out = Vec::with_capacity(len);
        out.set_len(len);
        generate_in(&mut out).await;
        out
    }
}

/// Fills `out` with randomly-generated data.
#[cfg(feature = "std")]
pub async fn generate_in(out: &mut [u8]) {
    for chunk in out.chunks_mut(usize::from(u16::max_value())) {
        let msg = ffi::RandomMessage::Generate {
            len: u16::try_from(chunk.len()).unwrap(),
        };
        let rep: ffi::GenerateResponse = unsafe {
            redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg)
                .unwrap()
                .await
        };
        chunk.copy_from_slice(&rep.result);
    }
}
