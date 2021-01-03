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

//! Core system that includes executing Wasm programs passing messages to each other.
//!
//! This module is lower-level than [`system`](super::system). It doesn't hardcode any interface.

mod extrinsics;
mod ipc;
mod processes;
mod tests;
mod vm;

pub use self::ipc::{Core, CoreBuilder, CoreProcess, CoreRunOutcome};
pub use self::vm::NewErr;
