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

use async_std::{
    net::TcpStream,
    sync::Mutex,
    task,
};
use fnv::FnvHashMap;
use futures::{channel::mpsc, prelude::*};
use std::{
    collections::hash_map::Entry,
    fmt,
    net::{Ipv6Addr, SocketAddr},
};

/// State machine for all TCP/IP connections that use the host operating system.
///
/// # Usage
///
/// Create a new [`TcpState`] using [`TcpState::new`]. Call [`TcpState::handle_message`] for each
/// message that a process sends on the TCP interface. In parallel, call [`TcpState::next_event`]
/// in order to receive answers to send back to processes.
///
pub struct TcpState {
    /// Receives messages from the sockets background tasks.
    receiver: Mutex<mpsc::Receiver<BackToFront>>,

    /// List of all active sockets. Contains both open and non-open sockets.
    sockets: Mutex<FnvHashMap<u32, FrontSocketState>>,

    /// Sending side of `receiver`. Meant to be cloned and sent to background tasks.
    sender: mpsc::Sender<BackToFront>,
}

/// State of a socket known from the front state.
struct FrontSocketState {
    /// If the socket is connected, a sender to send commands to the background task. `None` if
    /// the socket is not connected yet.
    sender: Option<mpsc::Sender<FrontToBack>>,
}

#[derive(Debug)]
pub enum TcpResponse {
    Open(u64, nametbd_tcp_interface::ffi::TcpOpenResponse),
    Read(u64, nametbd_tcp_interface::ffi::TcpReadResponse),
    Write(u64, nametbd_tcp_interface::ffi::TcpWriteResponse),
}

/// Message sent from the main task to the background task.
enum FrontToBack {
    Read { message_id: u64 },
    Write { message_id: u64, data: Vec<u8> },
}

/// Message sent from a background socket task to the main task.
enum BackToFront {
    OpenOk {
        open_message_id: u64,
        socket_id: u32,
        sender: mpsc::Sender<FrontToBack>,
    },
    OpenErr {
        open_message_id: u64,
        socket_id: u32,
    },
    Read {
        message_id: u64,
        result: Result<Vec<u8>, ()>,
    },
    Write {
        message_id: u64,
        result: Result<(), ()>,
    },
}

impl TcpState {
    /// Initializes a new empty [`TcpState`].
    pub fn new() -> TcpState {
        let (sender, receiver) = mpsc::channel(32);

        TcpState {
            sockets: Mutex::new(FnvHashMap::default()),
            receiver: Mutex::new(receiver),
            sender,
        }
    }

    /// Injects a message from a process into the state machine.
    ///
    /// Call [`TcpState::next_event`] in order to receive a response (if relevant).
    pub async fn handle_message(
        &self,
        //emitter_pid: u64,     // TODO: also notify the TcpState when a process exits, for clean up
        message_id: Option<u64>,
        message: nametbd_tcp_interface::ffi::TcpMessage,
    ) {
        let mut sockets = self.sockets.lock().await;

        match message {
            nametbd_tcp_interface::ffi::TcpMessage::Open(open) => {
                let message_id = message_id.unwrap(); // TODO: don't unwrap; but what to do?
                let socket_addr = {
                    let ip_addr = Ipv6Addr::from(open.ip);
                    if let Some(ip_addr) = ip_addr.to_ipv4() {
                        SocketAddr::new(ip_addr.into(), open.port)
                    } else {
                        SocketAddr::new(ip_addr.into(), open.port)
                    }
                };

                // Find a vacant entry in `self.sockets`, spawn the task, and insert.
                let mut tentative_socket_id = rand::random();
                loop {
                    match sockets.entry(tentative_socket_id) {
                        Entry::Occupied(_) => {
                            tentative_socket_id = tentative_socket_id.wrapping_add(1);
                            continue;
                        }
                        Entry::Vacant(e) => {
                            task::spawn(socket_task(
                                tentative_socket_id,
                                message_id,
                                socket_addr,
                                self.sender.clone(),
                            ));
                            e.insert(FrontSocketState { sender: None });
                            break;
                        }
                    }
                }
            }

            nametbd_tcp_interface::ffi::TcpMessage::Close(close) => {
                let _ = sockets.remove(&close.socket_id);
            }

            nametbd_tcp_interface::ffi::TcpMessage::Read(read) => {
                let message_id = message_id.unwrap(); // TODO: don't unwrap; but what to do?
                sockets
                    .get_mut(&read.socket_id)
                    .unwrap() // TODO: don't unwrap; but what to do?
                    .sender
                    .as_mut()
                    .unwrap()
                    .send(FrontToBack::Read { message_id })
                    .await
                    .unwrap(); // TODO: don't unwrap; but what to do?
            }

            nametbd_tcp_interface::ffi::TcpMessage::Write(write) => {
                let message_id = message_id.unwrap(); // TODO: don't unwrap; but what to do?
                sockets
                    .get_mut(&write.socket_id)
                    .unwrap() // TODO: don't unwrap; but what to do?
                    .sender
                    .as_mut()
                    .unwrap()
                    .send(FrontToBack::Write {
                        message_id,
                        data: write.data,
                    })
                    .await
                    .unwrap(); // TODO: don't unwrap; but what to do?
            }
        }
    }

    /// Returns the next message to respond to, and the response.
    pub async fn next_event(&self) -> TcpResponse {
        let message = {
            let mut receiver = self.receiver.lock().await;
            receiver.next().await.unwrap()
        };

        match message {
            BackToFront::OpenOk {
                open_message_id,
                socket_id,
                sender,
            } => {
                let mut sockets = self.sockets.lock().await;
                let mut front_state = sockets.get_mut(&socket_id).unwrap();
                debug_assert!(front_state.sender.is_none());
                front_state.sender = Some(sender);

                TcpResponse::Open(
                    open_message_id,
                    nametbd_tcp_interface::ffi::TcpOpenResponse {
                        result: Ok(socket_id),
                    },
                )
            }

            BackToFront::OpenErr {
                open_message_id,
                socket_id,
            } => {
                let mut sockets = self.sockets.lock().await;
                let _front_state = sockets.remove(&socket_id);
                debug_assert!(match _front_state {
                    Some(s) if s.sender.is_none() => true,
                    _ => false,
                });

                TcpResponse::Open(
                    open_message_id,
                    nametbd_tcp_interface::ffi::TcpOpenResponse { result: Err(()) },
                )
            }

            BackToFront::Read { message_id, result } => TcpResponse::Read(
                message_id,
                nametbd_tcp_interface::ffi::TcpReadResponse { result },
            ),

            BackToFront::Write { message_id, result } => TcpResponse::Write(
                message_id,
                nametbd_tcp_interface::ffi::TcpWriteResponse { result },
            ),
        }
    }
}

impl Default for TcpState {
    fn default() -> Self {
        TcpState::new()
    }
}

impl fmt::Debug for TcpState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("TcpState").finish()
    }
}

/// Function executed in the background for each TCP socket.
async fn socket_task(
    socket_id: u32,
    open_message_id: u64,
    socket_addr: SocketAddr,
    mut back_to_front: mpsc::Sender<BackToFront>,
) {
    // First step is to try connect to the destination.
    let (mut socket, mut commands_rx) = match TcpStream::connect(socket_addr).await {
        Ok(s) => {
            let (tx, rx) = mpsc::channel(2);
            let msg_to_front = BackToFront::OpenOk {
                socket_id,
                open_message_id,
                sender: tx,
            };

            if back_to_front.send(msg_to_front).await.is_err() {
                return;
            }

            (s, rx)
        }
        Err(_) => {
            let msg_to_front = BackToFront::OpenErr {
                socket_id,
                open_message_id,
            };
            let _ = back_to_front.send(msg_to_front).await;
            return;
        }
    };

    // Now that we're connected and we have a `socket` and `commands_rx`, we can start reading
    // and writing.
    loop {
        // TODO: should read and write asynchronously, but that's hard because of borrowing question
        match commands_rx.next().await {
            Some(FrontToBack::Read { message_id }) => {
                let mut read_buf = vec![0; 1024];
                let result = socket
                    .read(&mut read_buf)
                    .await
                    .map(|n| {
                        read_buf.truncate(n);
                        read_buf
                    })
                    .map_err(|_| ());
                let msg_to_front = BackToFront::Read { message_id, result };
                if back_to_front.send(msg_to_front).await.is_err() {
                    return;
                }
            }
            Some(FrontToBack::Write { message_id, data }) => {
                let result = socket.write_all(&data).await.map_err(|_| ());
                let msg_to_front = BackToFront::Write { message_id, result };
                if back_to_front.send(msg_to_front).await.is_err() {
                    return;
                }
            }
            None => {
                // `commands_rx` is closed, so let's stop the task.
                return;
            }
        }
    }
}
