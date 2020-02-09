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

#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::{convert::TryFrom, fmt, mem, ptr, slice, sync::atomic};
use futures::prelude::*;

#[allow(bad_style)]
mod bindings;
mod local_impl;

pub mod ffi;
