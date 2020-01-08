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

#![deny(intra_doc_link_resolution_failure)]
#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::{convert::TryFrom, fmt};
use futures::prelude::*;

pub use restricted::{RestrictedF32, RestrictedF64};

pub mod ffi;
mod restricted;

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

include!(concat!(env!("OUT_DIR"), "/webgpu.rs"));
