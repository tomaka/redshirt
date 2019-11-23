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

use parity_scale_codec::{Decode, Encode};

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: [u8; 32] = [
    0x19, 0x97, 0x70, 0x2f, 0x6f, 0xd9, 0x52, 0xcd, 0xb2, 0xc3, 0x75, 0x1c, 0x11, 0xb4, 0x95, 0x41,
    0x81, 0xa6, 0x4f, 0x91, 0x67, 0x63, 0xb5, 0xb1, 0x8d, 0x31, 0xdf, 0xb1, 0x47, 0x03, 0xa6, 0xbf,
];

#[derive(Debug, Encode, Decode)]
pub enum TimeMessage {
    /// Must respond with a `u128`.
    GetMonotonic,
    /// Must respond with a `u128`.
    GetSystem,
    /// Send response when the monotonic clock reaches this value. Responds with nothing (`()`).
    WaitMonotonic(u128),
}
