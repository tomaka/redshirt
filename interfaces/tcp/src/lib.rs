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

//! TCP/IP sockets.
//!
//! Allows opening asynchronous TCP sockets and listeners, similar to what the `tokio` or
//! `async-std` libraries do.
//!
//! See [the Wikipedia page](https://en.wikipedia.org/wiki/TCP/IP) for an introduction to TCP/IP.
//!
//! # Socket state
//!
//! At any given time, a TCP socket is in one of the following states:
//!
//! - Connecting/Listening. The socket is performing the three-way handshake or, for listening
//! sockets, is waiting for an incoming connection. From that state, a connection can transition
//! to the Established state.
//! - Established. The socket is connected and performing normal reads and writes.
//! - Closed wait. The socket has received a FIN from the remote. In this state, it is guaranteed
//! that reading from the socket will not produce any more data. Writing is still possible.
//! Depending on the logic of the application, the local machine might be encouraged to close
//! their side as soon as possible, or can continue writing data on the socket for a long period
//! of time.
//! - Fin wait. Our side has sent a FIN to the remote. This only happens if the application layer
//! has requested so. Writing is no longer allowed. Reading is still allowed and might yield more
//! data.
//! - Fin wait 2. The remote has ACK'ed the FIN that we have sent to it. Only happens after
//! the Fin wait state.
//! - Last ACK. The socket has received a FIN from the remote, and we have sent a FIN to the remote
//! as well, but the remote still has to ACK it.
//! - Finished. Both sides have sent a FIN to each other. Writing is forbidden, and reading is
//! guaranteed to no longer produce any data. The only sensible thing that can be done with the
//! socket is to destroy it.
//!
//! +---------------+       +---------------+      +---------------+
//! |  Connecting   |+----->|  Established  |+---->|  Closed wait  |
//! +---------------+       +---------------+      +---------------+
//!                                 | Close                | Close
//!                                 v                      v
//!                         +---------------+      +---------------+
//!                         |   Fin wait    |+---->|   Last ACK    |
//!                         +---------------+      +---------------+
//!                                 |                      |
//!                                 v                      v
//!                         +---------------+      +---------------+
//!                         |  Fin wait 2   |+---->|   Finished    |
//!                         +---------------+      +---------------+
//!
//! Additionally, the connection can jump at any point to the "Finished" state without any prior
//! warning, for example if a RST packet is received or if a protocol error is detected.
//!
//! > **Note**: The official denomination of the "Finished" state is "CLOSED", but we chose the
//! >           word "Finished" to clear any confusion regarding the relationship with the action
//! >           of sending a FIN packet.
//!
//! From the point of view of the user of this interface, all the state transitions happen
//! automatically except for the transitions from "Established" to "Fin wait" and from "Closed
//! wait" to "Last ACK", which happen when they request to close the socket.
//!
//! ## About listening sockets
//!
//! Contrary to Berkley sockets, there is no such thing as a listening socket that *accepts*
//! incoming connections and produces other sockets.
//!
//! Instead, you are expected to create multiple listening sockets that all listen on the same
//! port. When a remote tries to connect, one of these sockets transitions to the `Established`
//! state and is now considered connected to that the remote.
//!

use futures::{lock::Mutex, prelude::*, ready};
use redshirt_syscalls::{Encode as _, MessageResponseFuture};
use std::{
    cmp, io, mem,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
};

pub mod ffi;

/// Active TCP connection to a remote.
///
/// This type is similar to [`std::net::TcpStream`].
pub struct TcpStream {
    handle: u32,
    /// Buffer of data that has been read from the socket but not transmitted to the user yet.
    /// Contains `None` after the remote has sent us a FIN, meaning that we will not get any more
    /// data.
    read_buffer: Option<Vec<u8>>,
    /// If Some, we have sent out a "read" message and are waiting for a response.
    pending_read: Option<MessageResponseFuture<ffi::TcpReadResponse>>,
    /// If Some, we have sent out a "write" message and are waiting for a response.
    pending_write: Option<MessageResponseFuture<ffi::TcpWriteResponse>>,
    /// If Some, we have sent out a "close" message and are waiting for a response.
    pending_close: Option<MessageResponseFuture<ffi::TcpCloseResponse>>,
}

/// Active TCP listening socket.
///
/// This type is similar to [`std::net::TcpListener`].
pub struct TcpListener {
    local_addr: SocketAddr,
    next_incoming: Mutex<
        stream::FuturesUnordered<
            Pin<Box<dyn Future<Output = Result<(TcpStream, SocketAddr), ()>> + Send>>,
        >,
    >,
}

impl TcpStream {
    /// Start connecting to the given address. Returns a `TcpStream` if the connection is
    /// successful. The returned `TcpStream` is in the "Established" state (but might quickly
    /// transition to another state).
    pub fn connect(socket_addr: &SocketAddr) -> impl Future<Output = Result<TcpStream, ()>> {
        let fut = TcpStream::new(socket_addr, false);
        async move { Ok(fut.await?.0) }
    }

    /// Dialing and listening use the same underlying messages. The only different being a boolean
    /// indicating whether the address is a binding point or a destination.
    fn new(
        socket_addr: &SocketAddr,
        listen: bool,
    ) -> impl Future<Output = Result<(TcpStream, SocketAddr), ()>> {
        let tcp_open = ffi::TcpMessage::Open(match socket_addr {
            SocketAddr::V4(addr) => ffi::TcpOpen {
                ip: addr.ip().to_ipv6_mapped().segments(),
                port: addr.port(),
                listen,
            },
            SocketAddr::V6(addr) => ffi::TcpOpen {
                ip: addr.ip().segments(),
                port: addr.port(),
                listen,
            },
        });

        // Send the opening message here, so that the socket starts connecting or listening to
        // connections before we start polling the returned `Future`.
        let open_future = unsafe {
            let msg = tcp_open.encode();
            redshirt_syscalls::MessageBuilder::new()
                .add_data(&msg)
                .emit_with_response(&ffi::INTERFACE)
                .unwrap()
        };

        async move {
            let message: ffi::TcpOpenResponse = open_future.await;

            let socket_open_info = message.result?;
            let remote_addr = {
                let ip = Ipv6Addr::from(socket_open_info.remote_ip);
                SocketAddr::new(IpAddr::from(ip), socket_open_info.remote_port)
            };

            let stream = TcpStream {
                handle: socket_open_info.socket_id,
                read_buffer: Some(Vec::new()),
                pending_read: None,
                pending_write: None,
                pending_close: None,
            };

            Ok((stream, remote_addr))
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
                self.read_buffer = match ready!(Future::poll(Pin::new(pending_read), cx)).result {
                    Ok(d) if d.is_empty() => None,
                    Ok(d) => Some(d),
                    Err(ffi::TcpReadError::ConnectionFinished) => {
                        return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
                    }
                    Err(ffi::TcpReadError::InvalidSocket) => unreachable!(),
                };
                self.pending_read = None;
            }

            debug_assert!(self.pending_read.is_none());

            let read_buffer = match self.read_buffer.as_mut() {
                Some(b) => b,
                // We have received a FIN. Returning EOF.
                None => return Poll::Ready(Ok(0)),
            };

            if !read_buffer.is_empty() {
                let to_copy = cmp::min(read_buffer.len(), buf.len());
                let mut tmp = mem::replace(read_buffer, Vec::new());
                *read_buffer = tmp.split_off(to_copy);
                buf[..to_copy].copy_from_slice(&tmp);
                return Poll::Ready(Ok(to_copy));
            }

            self.pending_read = {
                let tcp_read = ffi::TcpMessage::Read(ffi::TcpRead {
                    socket_id: self.handle,
                });

                let msg_id = unsafe {
                    let msg = tcp_read.encode();
                    redshirt_syscalls::MessageBuilder::new()
                        .add_data(&msg)
                        .emit_with_response_raw(&ffi::INTERFACE)
                        .unwrap()
                };

                Some(redshirt_syscalls::message_response(msg_id))
            };
        }
    }

    // TODO: implement poll_read_vectored
    // TODO: unsafe fn initializer(&self) -> Initializer { ... }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        ready!(AsyncWrite::poll_flush(self.as_mut(), cx))?;
        debug_assert!(self.pending_write.is_none());

        // Perform the write, and store into `self.pending_write` a future to when we can start
        // the next write.
        self.pending_write = {
            let tcp_write = ffi::TcpMessage::Write(ffi::TcpWrite {
                socket_id: self.handle,
                data: buf.to_vec(), // TODO: meh for cloning
            });

            let msg_id = unsafe {
                let msg = tcp_write.encode(); // TODO: meh because we clone data a second time here
                redshirt_syscalls::MessageBuilder::new()
                    .add_data(&msg)
                    .emit_with_response_raw(&ffi::INTERFACE)
                    .unwrap()
            };

            Some(redshirt_syscalls::message_response(msg_id))
        };

        Poll::Ready(Ok(buf.len()))
    }

    // TODO: implement poll_write_vectored

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // Try to finish the previous write, if any is in progress.
        if let Some(pending_write) = self.pending_write.as_mut() {
            let result = ready!(Future::poll(Pin::new(pending_write), cx)).result;
            self.pending_write = None;
            match result {
                Ok(()) => Poll::Ready(Ok(())),
                Err(ffi::TcpWriteError::FinAlreaySent) => {
                    Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
                }
                Err(ffi::TcpWriteError::ConnectionFinished) => {
                    Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
                }
                Err(ffi::TcpWriteError::InvalidSocket) => unreachable!(),
            }
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // Try to finish the previous write, if any is in progress.
        if let Some(pending_write) = self.pending_write.as_mut() {
            let result = ready!(Future::poll(Pin::new(pending_write), cx)).result;
            self.pending_write = None;
            match result {
                Ok(()) => {}
                Err(ffi::TcpWriteError::FinAlreaySent) => return Poll::Ready(Ok(())),
                Err(ffi::TcpWriteError::ConnectionFinished) => {
                    return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
                }
                Err(ffi::TcpWriteError::InvalidSocket) => unreachable!(),
            }
        }

        debug_assert!(self.pending_write.is_none());

        loop {
            // Try to finish the previous close, if any is in progress.
            if let Some(pending_close) = self.pending_close.as_mut() {
                let result = ready!(Future::poll(Pin::new(pending_close), cx)).result;
                self.pending_close = None;
                match result {
                    Ok(()) | Err(ffi::TcpCloseError::FinAlreaySent) => return Poll::Ready(Ok(())),
                    Err(ffi::TcpCloseError::ConnectionFinished) => {
                        return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
                    }
                    Err(ffi::TcpCloseError::InvalidSocket) => unreachable!(),
                }
            }

            debug_assert!(self.pending_close.is_none());

            self.pending_close = {
                let tcp_close = ffi::TcpMessage::Close(ffi::TcpClose {
                    socket_id: self.handle,
                });

                let msg_id = unsafe {
                    redshirt_syscalls::MessageBuilder::new()
                        .add_data(&tcp_close.encode())
                        .emit_with_response_raw(&ffi::INTERFACE)
                        .unwrap()
                };

                Some(redshirt_syscalls::message_response(msg_id))
            };
        }
    }
}

impl tokio::io::AsyncRead for TcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let result = AsyncRead::poll_read(self, cx, buf.initialize_unfilled());
        if let Poll::Ready(Ok(n)) = result {
            buf.advance(n);
        }
        result.map_ok(|_| ())
    }
}

impl tokio::io::AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(self, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(self, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(self, cx)
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        unsafe {
            let destroy = ffi::TcpMessage::Destroy(self.handle);
            let _ = redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &destroy);
        }
    }
}

impl TcpListener {
    /// Create a new [`TcpListener`] listening on the given address and port.
    pub fn bind(socket_addr: &SocketAddr) -> impl Future<Output = Result<TcpListener, ()>> {
        let next_incoming = Mutex::new(
            (0..10)
                .map(|_| Box::pin(TcpStream::new(socket_addr, true)) as Pin<Box<_>>)
                .collect(),
        );

        let socket_addr = socket_addr.clone();
        async move {
            Ok(TcpListener {
                local_addr: socket_addr,
                next_incoming,
            })
        }
    }

    /// Returns the local address of the listener. Useful to determine the port.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Waits for a new incoming connection and returns it.
    pub async fn accept(&self) -> (TcpStream, SocketAddr) {
        let mut next_incoming = self.next_incoming.lock().await;

        let (tcp_stream, remote_addr) = loop {
            match next_incoming.next().await {
                Some(Ok(v)) => break v,
                Some(Err(_)) => continue,
                None => unreachable!(),
            }
        };

        next_incoming.push(Box::pin(TcpStream::new(&self.local_addr, true)));
        (tcp_stream, remote_addr)
    }
}
