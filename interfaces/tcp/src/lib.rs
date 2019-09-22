// Copyright(c) 2019 Pierre Krieger

//! TCP/IP.

#![deny(intra_doc_link_resolution_failure)]

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

        let event = syscalls::emit_message(&ffi::INTERFACE, &tcp_open, true);

        TcpStream {
            handle: 0,      // FIXME:
        }
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        let tcp_close = ffi::TcpClose {
            socket_id: self.handle,
        };

        syscalls::emit_message(&ffi::INTERFACE, &tcp_close, false);
    }
}
