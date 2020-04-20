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

//! Kernel logging.
//!
//! This interface permits two things:
//!
//! - Send logs for the kernel to print.
//! - Configure how the kernel prints logs.
//!
//! It is important for modules such as video drivers to keep the kernel up-to-date with how
//! logs should be displayed. In the case of a kernel panic, the kernel will use the information
//! contained in the latest received message in order to show diagnostics to the user.

#![no_std]

extern crate alloc;

use parity_scale_codec::Encode as _;

pub mod ffi;
pub use ffi::KernelLogMethod;

/// Appends a single ASCII string to the kernel logs.
///
/// This function always adds a single entry to the logs. An entry can made up of multiple lines
/// (separated with `\n`), but the lines are notably *not* split into multiple entries.
///
/// > **Note**: The message is expected to be in ASCII. It will otherwise be considered invalid
/// >           and get discarded.
///
/// # About `\r` vs `\n`
///
/// In order to follow the Unix world, the character `\n` (LF, 0xA) means "new line". The
/// character `\r` (CR, 0xD) is ignored.
///
pub fn log(msg: &[u8]) {
    unsafe {
        redshirt_syscalls::MessageBuilder::new()
            .add_data_raw(&[0][..])
            .add_data_raw(msg)
            .emit_without_response(&ffi::INTERFACE)
            .unwrap();
    }
}

/// Sets how the kernel should log messages.
pub async fn configure_kernel(method: KernelLogMethod) {
    unsafe {
        let encoded = method.encode();
        redshirt_syscalls::MessageBuilder::new()
            .add_data_raw(&[1][..])
            .add_data_raw(&encoded)
            .emit_with_response::<()>(&ffi::INTERFACE)
            .unwrap()
            .await;
    }
}
