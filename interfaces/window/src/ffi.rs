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
use redshirt_syscalls_interface::InterfaceHash;

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0xf6, 0x64, 0xe3, 0xe1, 0x50, 0x82, 0xa2, 0xc5, 0x13, 0x47, 0xc2, 0x29, 0xe9, 0x88, 0x4e, 0x50,
    0x97, 0xdf, 0xfd, 0xec, 0x4e, 0x41, 0x46, 0x2d, 0x12, 0xb2, 0xcc, 0xe3, 0x6b, 0x4c, 0xdd, 0xdd,
]);

// TODO: fullscreen-ness

#[derive(Debug, Encode, Decode)]
pub enum WindowMessage {
    Open(WindowOpen),
    Close(WindowClose),
    GetEvents(Vec<WindowEvent>),
}

#[derive(Debug, Encode, Decode)]
pub struct WindowOpen {}

#[derive(Debug, Encode, Decode)]
pub struct WindowOpenResponse {
    pub result: Result<u32, ()>,
}

#[derive(Debug, Encode, Decode)]
pub struct WindowClose {
    pub window_id: u32,
}

#[derive(Debug, Encode, Decode)]
pub enum WindowEvent {
    // TODO: it's very hipster to not provide the size of the window, but I don't know how viable that is
    Resized,
    // TODO: mouse events
    // TODO:
}
