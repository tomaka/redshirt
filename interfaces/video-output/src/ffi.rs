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

use parity_scale_codec::{Decode, Encode};
use redshirt_syscalls::InterfaceHash;

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0x87, 0x9d, 0xe0, 0xda, 0x61, 0x13, 0x26, 0x1d, 0x1f, 0x3e, 0xfa, 0x79, 0x4c, 0x9e, 0xa4, 0x67,
    0xf2, 0x81, 0xe8, 0x00, 0x39, 0x5e, 0xbe, 0x94, 0x1e, 0x49, 0xb8, 0xf8, 0xd4, 0x3b, 0x07, 0xce,
]);

#[derive(Debug, Encode, Decode)]
pub enum VideoOutputMessage {
    /// Notify of the existence of a new video output.
    // TODO: what if this id was already registered?
    Register {
        /// Unique per-process identifier.
        id: u64,
        /// Width in pixels of the output.
        width: u32,
        /// Height in pixels of the output.
        height: u32,
        /// Expected format of the output.
        format: Format,
    },

    /// Removes a previously-registered video output.
    Unregister(u64),

    /// Asks for the next image to present on this output. Must answer with a `NextImage`.
    NextImage(u64),
}

#[derive(Debug, Encode, Decode, Clone)]
pub struct NextImage {
    pub changes: Vec<NextImageChange>,
}

#[derive(Debug, Encode, Decode, Clone)]
pub struct NextImageChange {
    pub screen_x_start: u32,
    pub screen_x_len: u32,
    pub screen_y_start: u32,
    /// Rows of pixels.
    pub pixels: Vec<Vec<u8>>,
}

#[derive(Debug, Encode, Decode, Clone)]
pub enum Format {
    R8G8B8X8,
}
