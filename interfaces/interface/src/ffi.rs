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
    0x49, 0x6e, 0x56, 0x14, 0x8c, 0xd4, 0x2b, 0xc3, 0x9b, 0x4e, 0xbf, 0x5e, 0xb6, 0x2c, 0x60, 0x4d,
    0x7d, 0xd5, 0x70, 0x92, 0x4d, 0x4f, 0x70, 0xdf, 0xb3, 0xda, 0xf6, 0xfe, 0xdc, 0x65, 0x93, 0x8a,
]);

#[derive(Debug, Encode, Decode)]
pub enum InterfaceMessage {
    Register(InterfaceHash),
}

#[derive(Debug, Encode, Decode)]
pub struct InterfaceRegisterResponse {
    pub result: Result<(), InterfaceRegisterError>,
}

#[derive(Debug, Encode, Decode)]
pub enum InterfaceRegisterError {
    /// There already exists a process registered for this interface.
    AlreadyRegistered,
}
