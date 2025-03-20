// Copyright (C) 2019-2021  Pierre Krieger
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
    0xc2, 0xf9, 0xf8, 0xc5, 0xd5, 0xcb, 0x84, 0xb5, 0xc5, 0xfe, 0x34, 0x1d, 0x21, 0xb2, 0xc3, 0x6f,
    0xed, 0xfb, 0x86, 0xd1, 0xdb, 0xd6, 0x76, 0x41, 0x07, 0x02, 0x49, 0xeb, 0xfe, 0x1b, 0xa7, 0xc4,
]);

#[derive(Debug, Encode, Decode)]
pub enum TimeMessage {
    /// Must respond with a `u128`.
    GetSystem,
}
