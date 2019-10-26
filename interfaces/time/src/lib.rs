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

//! Time.

#![deny(intra_doc_link_resolution_failure)]

pub mod ffi;

/// Returns the number of nanoseconds since an arbitrary point in time in the past.
pub async fn monotonic_clock() -> u128 {
    let msg = ffi::TimeMessage::GetMonotonic;
    nametbd_syscalls_interface::emit_message_with_response(ffi::INTERFACE, msg)
        .await
        .unwrap()
}

/// Returns the number of nanoseconds since the Epoch (January 1st, 1970 at midnight UTC).
pub async fn system_clock() -> u128 {
    let msg = ffi::TimeMessage::GetSystem;
    nametbd_syscalls_interface::emit_message_with_response(ffi::INTERFACE, msg)
        .await
        .unwrap()
}
