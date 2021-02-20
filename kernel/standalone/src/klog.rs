// Copyright (C) 2019-2021  Pierre Krieger
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

//! Kernel logs handling.
//!
//! This module handles the way the kernel prints logs. It provides the [`KLogger`] structure
//! that needs to be configured with a certain logging output method, and is then capable of
//! outputting logs.
//!
//! # Panic-free code
//!
//! The code within this module is designed to be as panic-free as possible. In other words, you
//! can assume that a [`KLogger`] will be capable of printing a panic message without itself
//! triggering a nested panic. In particular, none of the code within this module does any heap
//! allocation.

pub use logger::KLogger;
pub use native::KernelLogNativeProgram;

mod logger;
mod native;
mod video;
