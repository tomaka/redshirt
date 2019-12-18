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
#![no_std]

extern crate alloc;

use alloc::string::String;

pub mod ffi;

/// Sends a string to be printed on stdout.
///
/// # About `\r` vs `\n`
///
/// In order to follow the Unix world, the character `\n` (LF, 0xA) means "new line". The
/// character `\r` (CR, 0xD) is ignored.
pub fn stdout(msg: String) {
    unsafe {
        let msg = ffi::StdoutMessage::Message(msg);
        redshirt_syscalls_interface::emit_message(&ffi::INTERFACE, &msg, false).unwrap();
    }
}
