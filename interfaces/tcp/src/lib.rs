// Copyright(c) 2019 Pierre Krieger

//! TCP/IP.

#![deny(intra_doc_link_resolution_failure)]

// TODO: everything here is a draft

use futures::prelude::*;
use parity_scale_codec::{Encode as _, DecodeAll};
use std::{io, net::SocketAddr, pin::Pin, task::Context, task::Poll};

pub mod ffi;

pub struct TcpStream {
    handle: u32,
    /// If Some, we have sent out a "read" message and are waiting for a response.
    has_pending_read: Option<u64>,
    /// If Some, we have sent out a "write" message and are waiting for a response.
    has_pending_write: Option<u64>,
}

impl TcpStream {
    pub fn connect(socket_addr: &SocketAddr) -> TcpStream {
        let tcp_open = ffi::TcpMessage::Open(match socket_addr {
            SocketAddr::V4(addr) => ffi::TcpOpen {
                ip: addr.ip().to_ipv6_mapped().segments(),
                port: addr.port(),
            },
            SocketAddr::V6(addr) => ffi::TcpOpen {
                ip: addr.ip().segments(),
                port: addr.port(),
            },
        });

        let msg_id = syscalls::emit_message(&ffi::INTERFACE, &tcp_open, true).unwrap().unwrap();
        let msg = syscalls::next_message(&mut [msg_id], true).unwrap();
        let handle = match msg {
            // TODO: code style: improve syscall's API
            syscalls::Message::Response(syscalls::ffi::ResponseMessage { message_id, actual_data }) => {
                assert_eq!(message_id, msg_id);
                let msg: ffi::TcpOpenResponse = DecodeAll::decode_all(&actual_data).unwrap();
                msg.result.unwrap()
            },
            _ => unreachable!()
        };

        TcpStream {
            handle,
            has_pending_read: None,
            has_pending_write: None,
        }
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<Result<usize, io::Error>> {
        // TODO: for now we're always blocking because we don't have threads
        let tcp_read = ffi::TcpMessage::Read(ffi::TcpRead {
            socket_id: self.handle,
        });
        let msg_id = syscalls::emit_message(&ffi::INTERFACE, &tcp_read, true).unwrap().unwrap();
        let msg = syscalls::next_message(&mut [msg_id], true).unwrap();
        let result = match msg {
            // TODO: code style: improve syscall's API
            syscalls::Message::Response(syscalls::ffi::ResponseMessage { message_id, actual_data }) => {
                assert_eq!(message_id, msg_id);
                let msg: ffi::TcpReadResponse = DecodeAll::decode_all(&actual_data).unwrap();
                msg.result
            },
            _ => unreachable!()
        };

        let data = match result {
            Ok(d) => d,
            Err(_) => return Poll::Ready(Err(io::ErrorKind::Other.into()))      // TODO:
        };

        buf[..data.len()].copy_from_slice(&data);       // TODO: this just assumes that buf is large enough
        Poll::Ready(Ok(data.len()))
    }

    // TODO: unsafe fn initializer(&self) -> Initializer { ... }
}

impl AsyncWrite for TcpStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
        // TODO: for now we're always blocking because we don't have threads
        let tcp_write = ffi::TcpMessage::Write(ffi::TcpWrite {
            socket_id: self.handle,
            data: buf.to_vec(),
        });
        let msg_id = syscalls::emit_message(&ffi::INTERFACE, &tcp_write, true).unwrap().unwrap();
        let msg = syscalls::next_message(&mut [msg_id], true).unwrap();
        let result = match msg {
            // TODO: code style: improve syscall's API
            syscalls::Message::Response(syscalls::ffi::ResponseMessage { message_id, actual_data }) => {
                assert_eq!(message_id, msg_id);
                let msg: ffi::TcpWriteResponse = DecodeAll::decode_all(&actual_data).unwrap();
                msg.result
            },
            _ => unreachable!()
        };

        match result {
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(io::ErrorKind::Other.into()))      // TODO:
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        let tcp_close = ffi::TcpMessage::Close(ffi::TcpClose {
            socket_id: self.handle,
        });

        syscalls::emit_message(&ffi::INTERFACE, &tcp_close, false);
    }
}
