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

use alloc::string::String;
use parity_scale_codec::{Decode, Encode};

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: [u8; 32] = [
    0xa6, 0xbc, 0x8d, 0xc3, 0x43, 0xbd, 0xdd, 0x3b, 0x44, 0x2f, 0x06, 0x40, 0xa8, 0x40, 0xad, 0x4f,
    0x25, 0x57, 0x22, 0x91, 0x79, 0xc8, 0x16, 0x07, 0x6f, 0xab, 0xa9, 0xd6, 0x38, 0xca, 0x01, 0x8b,
];

#[derive(Debug, Encode, Decode)]
pub enum StdoutMessage {
    /// Send text to print on stdout.
    ///
    /// > **Note**: There's no concept of piping, and stdout is meant to be used only for
    /// >           interfacing with the user.
    Message(String),
}
