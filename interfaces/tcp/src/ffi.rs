// Copyright(c) 2019 Pierre Krieger

use core::ffi::c_void;
use parity_scale_codec::{Encode, Decode};

#[link(wasm_import_module = "tcptcptcp")]       // TODO: proper hash
extern "C" {
    pub(crate) fn tcp_open(params: *const c_void, params_len: u32) -> i32;
    pub(crate) fn tcp_close(params: *const c_void, params_len: u32) -> i32;
}

#[derive(Debug, Encode, Decode)]
pub struct TcpOpen {
    pub ip: [u16; 8],
    pub port: u16,
}

#[derive(Debug, Encode, Decode)]
pub struct TcpClose {
    pub socket_id: u32,
}
