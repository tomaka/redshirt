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

//! Time.

#![no_std]

extern crate alloc;

use futures::prelude::*;

pub mod ffi;

/// Returns the number of nanoseconds since the Epoch (January 1st, 1970 at midnight UTC).
pub fn system_clock() -> impl Future<Output = u128> {
    unsafe {
        let msg = ffi::TimeMessage::GetSystem;
        redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg).unwrap()
    }
}
