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
    pending_read: Option<Pin<Box<dyn Future<Output = ffi::TcpReadResponse> + Send>>>,
    /// If Some, we have sent out a "write" message and are waiting for a response.
    // TODO: use strongly typed Future here
    pending_write: Option<Pin<Box<dyn Future<Output = ffi::TcpWriteResponse> + Send>>>,
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

        let msg_id = nametbd_syscalls_interface::emit_message(&ffi::INTERFACE, &tcp_open, true)
            .unwrap()
            .unwrap();

        async move {
            let message: ffi::TcpOpenResponse =
                nametbd_syscalls_interface::message_response(msg_id).await;
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
            let msg_id = nametbd_syscalls_interface::emit_message(&ffi::INTERFACE, &tcp_read, true)
                .unwrap()
                .unwrap();
            self.pending_read = Some(Box::pin(nametbd_syscalls_interface::message_response(
                msg_id,
            )));
        }
    }

    // TODO: unsafe fn initializer(&self) -> Initializer { ... }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        if let Some(pending_write) = self.pending_write.as_mut() {
            match ready!(Future::poll(Pin::new(pending_write), cx)).result {
                Ok(()) => self.pending_write = None,
                Err(_) => return Poll::Ready(Err(io::ErrorKind::Other.into())), // TODO:
            }
        }

        let tcp_write = ffi::TcpMessage::Write(ffi::TcpWrite {
            socket_id: self.handle,
            data: buf.to_vec(),
        });
        let msg_id = nametbd_syscalls_interface::emit_message(&ffi::INTERFACE, &tcp_write, true)
            .unwrap()
            .unwrap();
        self.pending_write = Some(Box::pin(nametbd_syscalls_interface::message_response(
            msg_id,
        )));
        Poll::Ready(Ok(buf.len()))
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

        nametbd_syscalls_interface::emit_message(&ffi::INTERFACE, &tcp_close, false);
    }
}
