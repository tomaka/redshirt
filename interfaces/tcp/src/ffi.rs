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
    0x10, 0x19, 0x16, 0x2a, 0x2b, 0x0c, 0x41, 0x36, 0x4a, 0x20, 0x01, 0x51, 0x47, 0x38, 0x27, 0x08,
    0x4a, 0x3c, 0x1e, 0x07, 0x18, 0x1c, 0x27, 0x11, 0x55, 0x15, 0x1d, 0x5f, 0x22, 0x5b, 0x16, 0x20,
]);

#[derive(Debug, Encode, Decode)]
pub enum TcpMessage {
    Open(TcpOpen),
    /// Ask to close the socket. Replied with a [`TcpCloseResponse`].
    Close(TcpClose),
    /// Ask to read data from a socket. The response is a [`TcpReadResponse`].
    Read(TcpRead),
    /// Ask to write data to a socket. A response is sent back once written. For each socket, only
    /// one write can exist at any given point in time.
    Write(TcpWrite),
    /// Destroy the given socket. Doesn't expect any response. The given socket ID will no longer
    /// be valid, and any existing message be replied to with `InvalidSocket`.
    Destroy(u32),
}

#[derive(Debug, Encode, Decode)]
pub struct TcpOpen {
    /// If true, then `ip` and `port` designate a local IP and port that the socket must listen
    /// on. A response will arrive when a remote connects to this IP and port.
    ///
    /// If false, then `ip` and `port` designate a remote IP and port that the socket will try to
    /// connect to. A response will arrive when we successfully connect or fail to connect.
    pub listen: bool,
    /// IPv6 address.
    pub ip: [u16; 8],
    /// TCP port.
    pub port: u16,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpOpenResponse {
    // TODO: proper error type
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
pub struct TcpCloseResponse {
    pub result: Result<(), TcpCloseError>,
}

#[derive(Debug, Encode, Decode, derive_more::Display)]
pub enum TcpCloseError {
    /// We have already sent a FIN to the remote. It is invalid to send another one.
    /// This happens if the connection is in the "Fin wait", "Fin wait 2", or "Last ACK" states.
    FinAlreaySent,
    /// Connection is in the "Finished" state.
    ConnectionFinished,
    /// The socket ID is invalid.
    InvalidSocket,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpRead {
    pub socket_id: u32,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpReadResponse {
    /// If the connection is in the "Closed" wait or "Last ACK" state, it is known that no more
    /// data will be received and an empty `Vec` is returned. If the connection is in the
    /// "Finished" state, then [`TcpReadError::ConnectionFinished`] is returned.
    pub result: Result<Vec<u8>, TcpReadError>,
}

#[derive(Debug, Encode, Decode, derive_more::Display)]
pub enum TcpReadError {
    /// Connection is in the "Finished" state.
    ConnectionFinished,
    /// The socket ID is invalid.
    InvalidSocket,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpWrite {
    pub socket_id: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpWriteResponse {
    pub result: Result<(), TcpWriteError>,
}

#[derive(Debug, Encode, Decode, derive_more::Display)]
pub enum TcpWriteError {
    /// We have sent a FIN to the remote, and thus are not allowed to send any more data.
    /// This happens if the connection is in the "Fin wait", "Fin wait 2", or "Last ACK" states.
    FinAlreaySent,
    /// Connection is in the "Finished" state.
    ConnectionFinished,
    /// The socket ID is invalid.
    InvalidSocket,
}
