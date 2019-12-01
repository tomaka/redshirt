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

use alloc::vec::Vec;
use parity_scale_codec::{Decode, Encode};

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: [u8; 32] = [
    0xfb, 0x83, 0xa5, 0x46, 0xfd, 0xf1, 0x50, 0x7a, 0xef, 0x8c, 0xb6, 0x8f, 0xa5, 0x44, 0x49, 0x21,
    0x53, 0xe5, 0x83, 0xda, 0xf0, 0x66, 0xbc, 0x1a, 0xd2, 0x18, 0xfd, 0x00, 0x54, 0x7f, 0xdb, 0x25,
];

#[derive(Debug, Encode, Decode)]
pub enum RandomMessage {
    /// Ask to generate cryptographically-secure list of random numbers of the given length.
    ///
    /// The length is a `u16`, so the maximum size is 64kiB and there's no need to handle potential
    /// errors about the length being too long to fit in memory. Call multiple times to obtain
    /// more.
    Generate { len: u16 },
}

#[derive(Debug, Encode, Decode)]
pub struct GenerateResponse {
    /// Random bytes. Must be of the requested length.
    pub result: Vec<u8>,
}
