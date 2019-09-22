// Copyright(c) 2019 Pierre Krieger

use core::ffi::c_void;
use parity_scale_codec::{Encode, Decode};

#[derive(Debug, Encode, Decode)]
pub struct TcpOpen {
    pub ip: [u16; 8],
    pub port: u16,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpClose {
    pub socket_id: u32,
}
