// Copyright(c) 2019 Pierre Krieger

//! TCP/IP.

// TODO: everything here is a draft

use parity_scale_codec::{Encode as _};
use std::net::SocketAddr;

pub mod ffi;

pub struct TcpStream {
    handle: u32,
}

impl TcpStream {
    pub fn connect(socket_addr: &SocketAddr) -> TcpStream {
        let tcp_open = match socket_addr {
            SocketAddr::V4(addr) => ffi::TcpOpen {
                ip: addr.ip().to_ipv6_mapped().segments(),
                port: addr.port(),
            },
            SocketAddr::V6(addr) => ffi::TcpOpen {
                ip: addr.ip().segments(),
                port: addr.port(),
            },
        };

        let param_bytes = tcp_open.encode();
        let handle = unsafe { ffi::tcp_open(param_bytes.as_ptr() as *const _, param_bytes.len() as u32) } as u32;       // TODO: no, don't return as return value

        TcpStream {
            handle,
        }
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        let tcp_close = ffi::TcpClose {
            socket_id: self.handle,
        };

        let param_bytes = tcp_close.encode();
        unsafe { ffi::tcp_close(param_bytes.as_ptr() as *const _, param_bytes.len() as u32); }
    }
}
