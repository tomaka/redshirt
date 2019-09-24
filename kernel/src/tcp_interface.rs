// Copyright(c) 2019 Pierre Krieger

//! Implements the TCP interface.

use async_std::net::TcpStream;
use fnv::FnvHashMap;
use futures::{prelude::*, ready};
use std::{io, net::{SocketAddr, Ipv6Addr}, pin::Pin, task::Context, task::Poll};

pub struct TcpState {
    next_socket_id: u32,
    sockets: FnvHashMap<u32, TcpConnec>,
}

#[derive(Debug)]
pub enum TcpResponse {
    Open(u64, tcp::ffi::TcpOpenResponse),
    Read(u64, tcp::ffi::TcpReadResponse),
    Write(u64, tcp::ffi::TcpWriteResponse),
}

impl TcpState {
    pub fn new() -> TcpState {
        TcpState {
            next_socket_id: 1,
            sockets: FnvHashMap::default(),
        }
    }

    pub fn handle_message(&mut self, event_id: Option<u64>, message: tcp::ffi::TcpMessage) {
        match message {
            tcp::ffi::TcpMessage::Open(open) => {
                let event_id = event_id.unwrap();
                let ip_addr = Ipv6Addr::from(open.ip);
                let socket_addr = if let Some(ip_addr) = ip_addr.to_ipv4() {
                    SocketAddr::new(ip_addr.into(), open.port)
                } else {
                    SocketAddr::new(ip_addr.into(), open.port)
                };
                let socket_id = self.next_socket_id;
                self.next_socket_id += 1;
                let socket = TcpStream::connect(socket_addr);
                self.sockets.insert(socket_id, TcpConnec::Connecting(socket_id, event_id, Box::pin(socket)));
            },
            tcp::ffi::TcpMessage::Close(close) => {
                let _ = self.sockets.remove(&close.socket_id);
            },
            tcp::ffi::TcpMessage::Read(read) => {
                let event_id = event_id.unwrap();
                unimplemented!()
            },
            tcp::ffi::TcpMessage::Write(write) => {
                let event_id = event_id.unwrap();
                unimplemented!()
            },
        }
    }

    /// Returns the next message to respond to, and the response.
    pub async fn next_event(&mut self) -> TcpResponse {
        if self.sockets.is_empty() {
            futures::pending!()
        }

        let (ev, _, _) = future::select_all(self.sockets.values_mut().map(|tcp| tcp.next_event())).await;
        println!("answering with {:?}", ev);
        ev
    }
}

enum TcpConnec {
    Connecting(u32, u64, Pin<Box<dyn Future<Output = Result<TcpStream, io::Error>> + Send>>),
    Socket(u32, TcpStream),
    Poisoned,
}

impl TcpConnec {
    pub fn next_event<'a>(&'a mut self) -> impl Future<Output = TcpResponse> + 'a {
        future::poll_fn(move |cx| {
            let (new_self, event) = match self {
                TcpConnec::Connecting(id, event_id, ref mut fut) => {
                    match ready!(Future::poll(Pin::new(fut), cx)) {
                        Ok(socket) => {
                            let ev = TcpResponse::Open(*event_id, tcp::ffi::TcpOpenResponse {
                                result: Ok(*id)
                            });
                            (TcpConnec::Socket(*id, socket), ev)
                        }
                        Err(_) => {
                            let ev = TcpResponse::Open(*event_id, tcp::ffi::TcpOpenResponse {
                                result: Err(())
                            });
                            (TcpConnec::Poisoned, ev)
                        }
                    }
                },
                TcpConnec::Socket(_, _) => unimplemented!(),
                TcpConnec::Poisoned => panic!(),
            };

            *self = new_self;
            Poll::Ready(event)
        })
    }
}
