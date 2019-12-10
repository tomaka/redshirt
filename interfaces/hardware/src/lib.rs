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

//! Access to physical hardware.
//!
//! Use this interface if you're writing a device driver.

#![deny(intra_doc_link_resolution_failure)]
#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec};

pub mod ffi;

/// Writes the given data to the given physical memory address location.
pub unsafe fn write(address: u64, data: impl Into<Vec<u8>>) {
    let msg = ffi::HardwareMessage::HardwareAccess(vec![
        ffi::Operation::PhysicalMemoryWrite {
            address,
            data: data.into(),
        }
    ]);

    nametbd_syscalls_interface::emit_message_without_response(&ffi::INTERFACE, &msg).unwrap();
}
