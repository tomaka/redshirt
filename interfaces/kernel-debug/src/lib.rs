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

//! Gathering kernel metrics.
//!
//! This interface provides a way to gather information from the kernel. In particular, the kernel
//! returns [Prometheus](https://prometheus.io) metrics.
//!
//! # Message format
//!
//! - Sender sends a message with an empty body.
//! - Handler sends back a Prometheus-compatible UTF-8 message.
//!
//! See [this page](https://prometheus.io/docs/instrumenting/exposition_formats/#text-format-details)
//! for more information about the format.
//!

#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use redshirt_syscalls::InterfaceHash;

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0x8c, 0xb4, 0xc6, 0xee, 0xc9, 0x29, 0xc5, 0xce, 0xaf, 0x46, 0x28, 0x74, 0x9e, 0x96, 0x72, 0x58,
    0xea, 0xd2, 0xa2, 0xa2, 0xd6, 0xeb, 0x7d, 0xc8, 0xe3, 0x95, 0x0c, 0x0e, 0x53, 0xde, 0x8c, 0xba,
]);

/// Loads metrics from the kernel, as a Prometheus-compatible UTF-8 string.
pub async fn get_prometheus_metrics() -> String {
    unsafe {
        let response: redshirt_syscalls::EncodedMessage =
            redshirt_syscalls::emit_message_with_response(
                &INTERFACE,
                redshirt_syscalls::EncodedMessage(Vec::new()),
            )
            .unwrap()
            .await;

        String::from_utf8(response.0).unwrap()
    }
}
