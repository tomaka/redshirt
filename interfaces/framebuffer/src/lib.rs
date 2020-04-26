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

//! Framebuffer interface.
//!
//! Allows drawing an image.
//!
//! > **Note**: The fate of this interface is kind of vague. It is also unclear whether
//! >           keyboard/mouse input should be handled here as well. Use at your own risks.

#![no_std]

use core::convert::TryFrom as _;

pub mod ffi;

/// Framebuffer containing pixel data.
pub struct Framebuffer {
    id: u32,
    width: u32,
    height: u32,
}

impl Framebuffer {
    /// Initializes a new framebuffer of the given width and height.
    pub async fn new(width: u32, height: u32) -> Self {
        let id = unsafe {
            let mut out = [0; 4];
            redshirt_random_interface::generate_in(&mut out).await;
            u32::from_le_bytes(out)
        };

        unsafe {
            let id_le_bytes = id.to_le_bytes();
            let width_le_bytes = width.to_le_bytes();
            let height_le_bytes = height.to_le_bytes();
            redshirt_syscalls::MessageBuilder::new()
                .add_data_raw(&[0])
                .add_data_raw(&id_le_bytes[..])
                .add_data_raw(&width_le_bytes[..])
                .add_data_raw(&height_le_bytes[..])
                .emit_without_response(&ffi::INTERFACE)
                .unwrap();
        }

        Framebuffer { id, width, height }
    }

    /// Sets the data in the framebuffer.
    ///
    /// The size of `data` must be `width * height * 3`.
    pub fn set_data(&self, data: &[u8]) {
        unsafe {
            assert_eq!(
                data.len(),
                usize::try_from(
                    self.width
                        .checked_mul(self.height)
                        .unwrap()
                        .checked_mul(3)
                        .unwrap()
                )
                .unwrap()
            );

            let id_le_bytes = self.id.to_le_bytes();
            redshirt_syscalls::MessageBuilder::new()
                .add_data_raw(&[2])
                .add_data_raw(&id_le_bytes[..])
                .add_data_raw(data)
                .emit_without_response(&ffi::INTERFACE)
                .unwrap();
        }
    }

    // TODO: next_event() method
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        unsafe {
            let id_le_bytes = self.id.to_le_bytes();
            redshirt_syscalls::MessageBuilder::new()
                .add_data_raw(&[1])
                .add_data_raw(&id_le_bytes[..])
                .emit_without_response(&ffi::INTERFACE)
                .unwrap();
        }
    }
}
