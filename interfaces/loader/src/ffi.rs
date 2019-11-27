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
    0x1c, 0x72, 0xb5, 0x18, 0x7f, 0x73, 0x52, 0xfd, 0xf7, 0xa7, 0x81, 0xe2, 0xa8, 0x46, 0x51, 0xd7,
    0xb3, 0xc6, 0x2d, 0x24, 0x31, 0x88, 0x96, 0x95, 0x6e, 0xfc, 0x7d, 0x4d, 0x86, 0x3f, 0xff, 0xa6,
];

#[derive(Debug, Encode, Decode)]
pub enum LoaderMessage {
    Load([u8; 32]),
}

#[derive(Debug, Encode, Decode)]
pub struct LoadResponse {
    pub result: Result<Vec<u8>, ()>,
}
