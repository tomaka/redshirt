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

use parity_scale_codec::{Decode, Encode};

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: [u8; 32] = [
    0xf3, 0x93, 0x41, 0x2b, 0xbc, 0xc4, 0xe7, 0x9b, 0x2e, 0x36, 0x9c, 0x9c, 0xdd, 0xdf, 0xf0, 0xd9,
    0xb4, 0x9d, 0x28, 0x3c, 0x3b, 0x1a, 0x52, 0x8f, 0xf0, 0x0b, 0x0c, 0xbf, 0x61, 0x85, 0x5a, 0x0f,
];

#[derive(Debug, Encode, Decode)]
pub enum ThreadsMessage {
    New(ThreadNew),
    FutexWait(FutexWait),
    FutexWake(FutexWake),
}

#[derive(Debug, Encode, Decode)]
pub struct ThreadNew {
    /// Pointer to a function to start to execute in the new thread.
    ///
    /// The function must have a signature of the type `(U32) -> ()`. The parameter is the
    /// `user_data` below.
    // TODO: document more why it's a U32, as this is very WASM-specific
    pub fn_ptr: u32,
    /// Pointer to some user data that is passed as parameter to the function.
    pub user_data: u32,
}

// TODO: eventually these might be removed in favour of the native WASM atomic instructions:
// - https://doc.rust-lang.org/nightly/core/arch/wasm32/fn.atomic_notify.html
// - https://doc.rust-lang.org/nightly/core/arch/wasm32/fn.i32_atomic_wait.html
// - https://doc.rust-lang.org/nightly/core/arch/wasm32/fn.i64_atomic_wait.html

#[derive(Debug, Encode, Decode)]
pub struct FutexWait {
    /// Memory address of a 32bits opaque value.
    pub addr: u32,
    /// Value to compare with is what is pointed to by `addr`.
    pub val_cmp: u32,
}

#[derive(Debug, Encode, Decode)]
pub struct FutexWake {
    /// Memory address of a 32bits opaque value.
    pub addr: u32,
    /// Maximum number of threads to wake up. Generally, only the values `1` or `u32::max_value()`
    /// make sense.
    pub nwake: u32,
}
