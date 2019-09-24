// Copyright(c) 2019 Pierre Krieger

use parity_scale_codec::{Encode, Decode};

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: [u8; 32] = [
    0x10, 0x19, 0x16, 0x2a, 0x2b, 0x0c, 0x41, 0x36,
    0x4a, 0x20, 0x01, 0x51, 0x47, 0x38, 0x27, 0x08,
    0x4a, 0x3c, 0x1e, 0x07, 0x18, 0x1c, 0x27, 0x11,
    0x55, 0x15, 0x1d, 0x5f, 0x22, 0x5b, 0x16, 0x20,
];

#[derive(Debug, Encode, Decode)]
pub enum TcpMessage {
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
pub struct TcpOpen {
    pub ip: [u16; 8],
    pub port: u16,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpOpenResponse {
    pub result: Result<u32, ()>,
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
