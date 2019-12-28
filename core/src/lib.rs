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

#![warn(missing_docs)]
#![deny(unsafe_code)]
#![deny(intra_doc_link_resolution_failure)]
#![allow(dead_code)] // TODO: temporary during development
#![no_std]

extern crate alloc;

pub use self::module::Module;
pub use self::system::{System, SystemBuilder, SystemRunOutcome};
pub use redshirt_syscalls_interface::{
    Decode, Encode, EncodedMessage, InterfaceHash, MessageId, Pid, ThreadId,
};
pub use wasmi::RuntimeValue; // TODO: wrap around instead?

mod id_pool;

pub mod module;
pub mod native;
pub mod scheduler;
pub mod signature;
pub mod system;
