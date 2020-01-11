// Copyright (C) 2020  Pierre Krieger
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

//! WebGPU.
//!
//! The WebGPU API is a graphical rendering API. In other words, it is used in order to show
//! something on the screen.
//!
//! This documentation doesn't explain how to use this API. While WebGPU is a fairly complex API,
//! it is also expected that it becomes an official W3C standard, and that documentation and
//! tutorials about it become widely available.
//!
//! References:
//!
//! - https://en.wikipedia.org/wiki/WebGPU
//! - https://gpuweb.github.io/gpuweb/
//!
//! # Interfaces
//!
//! > **Note**: This is not implemented yet.
//!
//! Contrary to most other redshirt interfaces, there exists *two* interfaces related to WebGPU:
//! TODO: name them or something
//!
//! - The first one allows drawing to the entire screen. Only one process can use it at any given
//! point in time. TODO: what if multiple screens?
//! - The second one allows drawing on a window. Multiple processes can all create adapters and
//! devices and draw simultaneously, each on their own window.
//!
//! An implementation of the second one is typically expected to use the first one under the
//! hood.
//!
//! > **Note**: An implementation of the second one is typically called a windows compositor in
//! >           most operating systems.
//!

#![deny(intra_doc_link_resolution_failure)]
#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::{convert::TryFrom, fmt, sync::atomic};
use futures::prelude::*;

pub use restricted::{RestrictedF32, RestrictedF64};

pub mod ffi;
mod restricted;

/// Whenever we create a new object (e.g. a `GPUBuffer`), we decide locally of the ID of the
/// object and pass it to the interface implementer.
static NEXT_OBJECT_ID: atomic::AtomicU64 = atomic::AtomicU64::new(1);

/// Defined in the "ImageBitmap and animations" standard.
///
/// https://html.spec.whatwg.org/multipage/imagebitmap-and-animations.html#imagebitmap
///
/// There is no way to construct a [`ImageBitmap`] in this crate.
#[derive(Debug, parity_scale_codec::Encode, parity_scale_codec::Decode)]
pub struct ImageBitmap {
}

#[derive(Debug, parity_scale_codec::Encode, parity_scale_codec::Decode)]
pub struct ArrayBuffer {
}

pub struct Navigator {
}

pub struct WorkerNavigator {
}

// https://dom.spec.whatwg.org/#dictdef-eventinit
#[derive(Debug, parity_scale_codec::Encode, parity_scale_codec::Decode)]
pub struct EventInit {
    pub bubbles: bool,
    pub cancelable: bool,
    pub composed: bool,
}

pub const GPU: GPU = GPU { inner: 0 };      // TODO: hack
pub const GPUCanvasContext: GPUCanvasContext = GPUCanvasContext { inner: 0 };      // TODO: hack

include!(concat!(env!("OUT_DIR"), "/webgpu.rs"));
