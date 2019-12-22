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
    0x10, 0x19, 0x16, 0x2a, 0x2b, 0x0c, 0x41, 0x36, 0x4a, 0x20, 0x01, 0x51, 0x47, 0x38, 0x27, 0x08,
    0x4a, 0x3c, 0x1e, 0x07, 0x18, 0x1c, 0x27, 0x11, 0x55, 0x15, 0x1d, 0x5f, 0x22, 0x5b, 0x16, 0x20,
];

#[derive(Debug, Encode, Decode)]
pub enum TcpMessage {
    /// Request to open a TCP socket. The socket can either attempt to connect to a third party,
    /// or listen on a port and wait for a third party to connect.
    Open(TcpOpen),
    Close(TcpClose),
    /// Ask to read data from a socket. The response contains the data. For each socket, only one
    /// read can exist at any given point in time.
    Read(TcpRead),
    /// Ask to write data to a socket. A response is sent back once written. For each socket, only
    /// one write can exist at any given point in time.
    Write(TcpWrite),
    RegisterInterface {
        id: u64,
        mac_address: [u8; 6],
    },
    UnregisterInterface(u64),
    /// Notify when an interface has received data (e.g. from the Internet). Must answer with a
    /// `()` when the send is finished and we're ready to accept a new packet.
    InterfaceOnData(u64, Vec<u8>),
    /// Asks for the next packet of data to send out through this interface (e.g. going towards
    /// the Internet). Must answer with a `Vec<u8>`.
    InterfaceWaitData(u64),
}

#[derive(Debug, Encode, Decode)]
pub struct TcpOpen {
    /// If true, then `ip` and `port` designate a local IP and port that the socket must listen
    /// on. A response will arrive when a remote connects to this IP and port.
    ///
    /// If false, then `ip` and `port` designate a remote IP and port that the socket will try to
    /// connect to. A response will arrive when we successfully connect or fail to connect.
    // TODO: enum instead?
    pub listen: bool,
    /// IPv6 address.
    pub ip: [u16; 8],
    /// TCP port.
    pub port: u16,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpOpenResponse {
    pub result: Result<TcpSocketOpen, ()>,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpSocketOpen {
    pub socket_id: u32,
    pub local_ip: [u16; 8],
    pub local_port: u16,
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

#[derive(Debug, Encode, Decode)]
pub struct InterfaceOnDataResponse {
    pub result: Result<(), ()>,
}

#[derive(Debug, Encode, Decode)]
pub struct InterfaceWaitDataResponse {
    pub result: Result<Vec<u8>, ()>,
}
