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

//! Implements the TCP interface.

use async_std::net::TcpStream;
use fnv::FnvHashMap;
use futures::{prelude::*, ready};
use std::{
    io,
    net::{Ipv6Addr, SocketAddr},
    pin::Pin,
    task::Context,
    task::Poll,
};

pub struct TcpState {
    next_socket_id: u32,
    sockets: FnvHashMap<u32, TcpConnec>,
}

#[derive(Debug)]
pub enum TcpResponse {
    Open(u64, nametbd_tcp_interface::ffi::TcpOpenResponse),
    Read(u64, nametbd_tcp_interface::ffi::TcpReadResponse),
    Write(u64, nametbd_tcp_interface::ffi::TcpWriteResponse),
}

impl TcpState {
    pub fn new() -> TcpState {
        TcpState {
            next_socket_id: 1,
            sockets: FnvHashMap::default(),
        }
    }

    pub fn handle_message(&mut self, message_id: Option<u64>, message: nametbd_tcp_interface::ffi::TcpMessage) {
        match message {
            nametbd_tcp_interface::ffi::TcpMessage::Open(open) => {
                let message_id = message_id.unwrap();
                let ip_addr = Ipv6Addr::from(open.ip);
                let socket_addr = if let Some(ip_addr) = ip_addr.to_ipv4() {
                    SocketAddr::new(ip_addr.into(), open.port)
                } else {
                    SocketAddr::new(ip_addr.into(), open.port)
                };
                let socket_id = self.next_socket_id;
                self.next_socket_id += 1;
                let socket = TcpStream::connect(socket_addr);
                self.sockets.insert(
                    socket_id,
                    TcpConnec::Connecting(socket_id, message_id, Box::pin(socket)),
                );
            }
            nametbd_tcp_interface::ffi::TcpMessage::Close(close) => {
                let _ = self.sockets.remove(&close.socket_id);
            }
            nametbd_tcp_interface::ffi::TcpMessage::Read(read) => {
                let message_id = message_id.unwrap();
                self.sockets
                    .get_mut(&read.socket_id)
                    .unwrap()
                    .start_read(message_id);
            }
            nametbd_tcp_interface::ffi::TcpMessage::Write(write) => {
                let message_id = message_id.unwrap();
                self.sockets
                    .get_mut(&write.socket_id)
                    .unwrap()
                    .start_write(message_id, write.data);
            }
        }
    }

    /// Returns the next message to respond to, and the response.
    pub async fn next_event(&mut self) -> TcpResponse {
        // `select_all` panics if the list passed to it is empty, so we have to account for that.
        while self.sockets.is_empty() {
            futures::pending!()
        }

        let (ev, _, _) =
            future::select_all(self.sockets.values_mut().map(|tcp| tcp.next_event())).await;
        ev
    }
}

enum TcpConnec {
    Connecting(
        u32,
        u64,
        Pin<Box<dyn Future<Output = Result<TcpStream, io::Error>> + Send>>,
    ),
    Socket {
        socket_id: u32,
        tcp_stream: TcpStream,
        pending_read: Option<u64>,
        pending_write: Option<(u64, Vec<u8>)>,
    },
    Poisoned,
}

impl TcpConnec {
    pub fn start_read(&mut self, message_id: u64) {
        let pending_read = match self {
            TcpConnec::Socket {
                ref mut pending_read,
                ..
            } => pending_read,
            _ => panic!(),
        };

        assert!(pending_read.is_none());
        *pending_read = Some(message_id);
    }

    pub fn start_write(&mut self, message_id: u64, data: Vec<u8>) {
        let pending_write = match self {
            TcpConnec::Socket {
                ref mut pending_write,
                ..
            } => pending_write,
            _ => panic!(),
        };

        assert!(pending_write.is_none());
        *pending_write = Some((message_id, data));
    }

    pub fn next_event<'a>(&'a mut self) -> impl Future<Output = TcpResponse> + 'a {
        future::poll_fn(move |cx| {
            let (new_self, event) = match self {
                TcpConnec::Connecting(id, message_id, ref mut fut) => {
                    match ready!(Future::poll(Pin::new(fut), cx)) {
                        Ok(socket) => {
                            let ev = TcpResponse::Open(
                                *message_id,
                                nametbd_tcp_interface::ffi::TcpOpenResponse { result: Ok(*id) },
                            );
                            (
                                TcpConnec::Socket {
                                    socket_id: *id,
                                    tcp_stream: socket,
                                    pending_write: None,
                                    pending_read: None,
                                },
                                ev,
                            )
                        }
                        Err(_) => {
                            let ev = TcpResponse::Open(
                                *message_id,
                                nametbd_tcp_interface::ffi::TcpOpenResponse { result: Err(()) },
                            );
                            (TcpConnec::Poisoned, ev)
                        }
                    }
                }

                TcpConnec::Socket {
                    socket_id,
                    tcp_stream,
                    pending_read,
                    pending_write,
                } => {
                    let write_finished = if let Some((msg_id, data_to_write)) = pending_write {
                        if !data_to_write.is_empty() {
                            let num_written = ready!(AsyncWrite::poll_write(
                                Pin::new(tcp_stream),
                                cx,
                                &data_to_write
                            ))
                            .unwrap();
                            for _ in 0..num_written {
                                data_to_write.remove(0);
                            }
                        }
                        if data_to_write.is_empty() {
                            ready!(AsyncWrite::poll_flush(Pin::new(tcp_stream), cx)).unwrap();
                            Some(*msg_id)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(msg_id) = write_finished {
                        *pending_write = None;
                        return Poll::Ready(TcpResponse::Write(
                            msg_id,
                            nametbd_tcp_interface::ffi::TcpWriteResponse { result: Ok(()) },
                        ));
                    }

                    if let Some(msg_id) = pending_read.clone() {
                        let mut buf = [0; 1024];
                        let num_read =
                            ready!(AsyncRead::poll_read(Pin::new(tcp_stream), cx, &mut buf))
                                .unwrap();
                        *pending_read = None;
                        return Poll::Ready(TcpResponse::Read(
                            msg_id,
                            nametbd_tcp_interface::ffi::TcpReadResponse {
                                result: Ok(buf[..num_read].to_vec()),
                            },
                        ));
                    }

                    return Poll::Pending;
                }

                TcpConnec::Poisoned => panic!(),
            };

            *self = new_self;
            Poll::Ready(event)
        })
    }
}
