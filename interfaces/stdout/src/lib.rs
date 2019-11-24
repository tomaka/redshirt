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

//! Stdout.

#![deny(intra_doc_link_resolution_failure)]

pub mod ffi;

/// Sends a string to be printed on stdout.
pub fn stdout(msg: String) {
    let msg = ffi::StdoutMessage::Message(msg);
    nametbd_syscalls_interface::emit_message(&ffi::INTERFACE, &msg, false)
        .unwrap();
}
