// Copyright(c) 2019 Pierre Krieger

//! TCP/IP.

#![deny(intra_doc_link_resolution_failure)]

// TODO: everything here is a draft

use futures::{prelude::*, ready};
use parity_scale_codec::{DecodeAll, Encode as _};
use std::{io, mem, net::SocketAddr, pin::Pin, sync::Arc, task::Context, task::Poll, task::Waker};

pub mod ffi;

pub struct TcpStream {
    handle: u32,
    /// If Some, we have sent out a "read" message and are waiting for a response.
    // TODO: use strongly typed Future here
    pending_read: Option<Pin<Box<dyn Future<Output = ffi::TcpReadResponse>>>>,
    /// If Some, we have sent out a "write" message and are waiting for a response.
    // TODO: use strongly typed Future here
    pending_write: Option<Pin<Box<dyn Future<Output = ffi::TcpWriteResponse>>>>,
}

impl TcpStream {
    pub fn connect(socket_addr: &SocketAddr) -> impl Future<Output = TcpStream> {
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

        let msg_id = syscalls::emit_message(&ffi::INTERFACE, &tcp_open, true)
            .unwrap()
            .unwrap();

        async move {
            let response = syscalls::message_response(msg_id).await;
            let message: ffi::TcpOpenResponse = DecodeAll::decode_all(&response.actual_data).unwrap();
            let handle = message.result.unwrap();

            TcpStream {
                handle,
                pending_read: None,
                pending_write: None,
            }
        }
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        loop {
            if let Some(pending_read) = self.pending_read.as_mut() {
                println!("polling pending read");
                let data = match ready!(Future::poll(Pin::new(pending_read), cx)).result {
                    Ok(d) => d,
                    Err(_) => return Poll::Ready(Err(io::ErrorKind::Other.into())), // TODO:
                };

                self.pending_read = None;
                buf[..data.len()].copy_from_slice(&data); // TODO: this just assumes that buf is large enough
                return Poll::Ready(Ok(data.len()));
            }

            let tcp_read = ffi::TcpMessage::Read(ffi::TcpRead {
                socket_id: self.handle,
            });
            let msg_id = syscalls::emit_message(&ffi::INTERFACE, &tcp_read, true)
                .unwrap()
                .unwrap();
            self.pending_read = Some(Box::pin(async move {
                let msg = syscalls::message_response(msg_id).await;
                DecodeAll::decode_all(&msg.actual_data).unwrap()
            }));
        }
    }

    // TODO: unsafe fn initializer(&self) -> Initializer { ... }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        // TODO: for now we're always blocking because we don't have threads
        let tcp_write = ffi::TcpMessage::Write(ffi::TcpWrite {
            socket_id: self.handle,
            data: buf.to_vec(),
        });
        let msg_id = syscalls::emit_message(&ffi::INTERFACE, &tcp_write, true)
            .unwrap()
            .unwrap();
        let msg = syscalls::next_message(&mut [msg_id], true).unwrap();
        let result = match msg {
            // TODO: code style: improve syscall's API
            syscalls::Message::Response(syscalls::ffi::ResponseMessage {
                message_id,
                actual_data,
                ..
            }) => {
                assert_eq!(message_id, msg_id);
                let msg: ffi::TcpWriteResponse = DecodeAll::decode_all(&actual_data).unwrap();
                msg.result
            }
            _ => unreachable!(),
        };

        match result {
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(io::ErrorKind::Other.into())), // TODO:
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
