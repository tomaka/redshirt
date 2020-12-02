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

//! Native programs handling.
//!
//! A so-called "native program" is a piece of code that interfaces with the core system in the
//! same way as a Wasm program.
//! In other words, it is similar to a Wasm program but directly embedded in the kernel.
//!
//! This feature is useful in order to provide the primitive interfaces that couldn't otherwise be
//! implemented from within a Wasm VM, such as a timers, randomness, access to physical memory,
//! and so on.
//!
//! This module defines the [`NativeProgramRef`] trait that should be implemented on native
//! programs.

// TODO: native programs should be refactored by returning events out of `System` rather than injecting a trait

pub use self::collection::{
    NativeProgramsCollection, NativeProgramsCollectionEvent, NativeProgramsCollectionMessageIdWrite,
};
pub use self::traits::{
    DummyMessageIdWrite, NativeProgramEvent, NativeProgramMessageIdWrite, NativeProgramRef,
};

mod collection;
mod traits;
