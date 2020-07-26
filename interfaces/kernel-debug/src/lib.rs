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

//! Loading WASM modules.
//!
//! This interface is a bit special, as it is used by the kernel in order to load WASM modules.

#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};

pub mod ffi;

/// Loads metrics from the kernel, as a Prometheus-compatible UTF-8 string.
pub async fn get_metrics() -> String {
    unsafe {
        let response: redshirt_syscalls::EncodedMessage =
            redshirt_syscalls::emit_message_with_response(
                &ffi::INTERFACE,
                redshirt_syscalls::EncodedMessage(Vec::new()),
            )
            .unwrap()
            .await;

        String::from_utf8(response.0).unwrap()
    }
}
