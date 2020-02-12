// Copyright (C) 2020  Pierre Krieger
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

use alloc::{string::String, vec::Vec};
use parity_scale_codec::{Decode, Encode};
use redshirt_syscalls::InterfaceHash;

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0xa0, 0x66, 0x42, 0xfe, 0xc4, 0x4f, 0xaf, 0xab, 0xb7, 0x14, 0x7f, 0xb1, 0x37, 0xf9, 0xe9, 0x7f,
    0xae, 0x28, 0x5a, 0xbb, 0x35, 0x34, 0x95, 0x93, 0x00, 0x0c, 0x19, 0x18, 0xd3, 0x8b, 0x9b, 0x1f,
]);

#[derive(Debug, Encode, Decode)]
pub enum WebGpuMessage {
    /// Returns a `u64` identifying the adapter.
    ///
    /// Design notes: the identifier is chosen by the interface handler rather than the caller
    /// side in order to avoid potential collisions when choosing the identifier.
    RequestAdapter,
}
