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
    0x10, 0x19, 0x16, 0x2a, 0x2b, 0x0c, 0x41, 0x36, 0x4a, 0x20, 0x01, 0x51, 0x47, 0x38, 0x27, 0x08,
    0x4a, 0x3c, 0x1e, 0x07, 0x18, 0x1c, 0x27, 0x11, 0x55, 0x15, 0x1d, 0x5f, 0x22, 0x5b, 0x16, 0x20,
]);

#[derive(Debug, Encode, Decode)]
pub enum TcpMessage {
    Listen(TcpListen),
    Accept(TcpAccept),
    Open(TcpOpen),
    Close(TcpClose),
    /// Ask to read data from a socket. The response contains the data. For each socket, only one
    /// read can exist at any given point in time.
    Read(TcpRead),
    /// Ask to write data to a socket. A response is sent back once written. For each socket, only
    /// one write can exist at any given point in time.
    Write(TcpWrite),
}

#[derive(Debug, Encode, Decode)]
pub struct TcpListen {
    pub local_ip: [u16; 8],
    /// Can be 0 for auto-assign.
    pub port: u16,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpListenResponse {
    /// On success, the socket ID and the port it's listening on.
    pub result: Result<(u32, u16), ()>,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpOpen {
    pub ip: [u16; 8],
    pub port: u16,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpOpenResponse {
    pub result: Result<u32, ()>,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpAccept {
    pub socket_id: u32,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpAcceptResponse {
    pub accepted_socket_id: u32,
    pub remote_ip: [u16; 8],
    pub remote_port: u16,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpClose {
    pub socket_id: u32,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpRead {
    pub socket_id: u32,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpReadResponse {
    pub result: Result<Vec<u8>, ()>,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpWrite {
    pub socket_id: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpWriteResponse {
    pub result: Result<(), ()>,
}
