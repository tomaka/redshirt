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
    net::{TcpListener, TcpStream},
    sync::Mutex,
    task,
};
use fnv::FnvHashMap;
use futures::{channel::mpsc, prelude::*};
use parity_scale_codec::Encode as _;
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
pub struct TcpState<TMsgId> {
    /// Receives messages from the sockets background tasks.
    receiver: Mutex<mpsc::Receiver<BackToFront<TMsgId>>>,

    /// List of all active sockets. Contains both open and non-open sockets.
    sockets: Mutex<FnvHashMap<u32, FrontSocketState<TMsgId>>>,

    /// Sending side of `receiver`. Meant to be cloned and sent to background tasks.
    sender: mpsc::Sender<BackToFront<TMsgId>>,
}

/// State of a socket known from the front state.
enum FrontSocketState<TMsgId> {
    /// This socket ID is reserved, but not the background task is in the process of opening it.
    Orphan,

    /// The socket is connected. Contains a sender to send commands to the background task.
    Connected(mpsc::Sender<FrontToBackSocket<TMsgId>>),

    /// The socket is a listener.
    Listener(mpsc::Sender<FrontToBackListener<TMsgId>>),
}

/// Message sent from the main task to the background task for sockets.
enum FrontToBackSocket<TMsgId> {
    Read { message_id: TMsgId },
    Write { message_id: TMsgId, data: Vec<u8> },
}

/// Message sent from the main task to the background task for listeners.
enum FrontToBackListener<TMsgId> {
    Accept { message_id: TMsgId },
}

/// Message sent from a background socket task to the main task.
enum BackToFront<TMsgId> {
    OpenOk {
        open_message_id: TMsgId,
        socket_id: u32,
        sender: mpsc::Sender<FrontToBackSocket<TMsgId>>,
    },
    OpenErr {
        open_message_id: TMsgId,
        socket_id: u32,
    },
    ListenOk {
        listen_message_id: TMsgId,
        socket_id: u32,
        local_addr: SocketAddr,
        sender: mpsc::Sender<FrontToBackListener<TMsgId>>,
    },
    ListenErr {
        listen_message_id: TMsgId,
        socket_id: u32,
    },
    Read {
        message_id: TMsgId,
        result: Result<Vec<u8>, ()>,
    },
    Write {
        message_id: TMsgId,
        result: Result<(), ()>,
    },
    Accept {
        message_id: TMsgId,
        socket: TcpStream,
    },
}

impl<TMsgId> TcpState<TMsgId> {
    /// Initializes a new empty [`TcpState`].
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(32);

        TcpState {
            sockets: Mutex::new(FnvHashMap::default()),
            receiver: Mutex::new(receiver),
            sender,
        }
    }
}

impl<TMsgId> TcpState<TMsgId>
where
    TMsgId: Send + 'static,
{
    /// Injects a message from a process into the state machine.
    ///
    /// Call [`TcpState::next_event`] in order to receive a response (if relevant).
    pub async fn handle_message(
        &self,
        //emitter_pid: u64,     // TODO: also notify the TcpState when a process exits, for clean up
        message_id: Option<TMsgId>,
        message: redshirt_tcp_interface::ffi::TcpMessage,
    ) {
        let mut sockets = self.sockets.lock().await;

        match message {
            redshirt_tcp_interface::ffi::TcpMessage::Open(open) => {
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
                            e.insert(FrontSocketState::Orphan);
                            break;
                        }
                    }
                }
            }

            redshirt_tcp_interface::ffi::TcpMessage::Listen(listen) => {
                let message_id = message_id.unwrap(); // TODO: don't unwrap; but what to do?
                let socket_addr = {
                    let ip_addr = Ipv6Addr::from(listen.local_ip);
                    if let Some(ip_addr) = ip_addr.to_ipv4() {
                        SocketAddr::new(ip_addr.into(), listen.port)
                    } else {
                        SocketAddr::new(ip_addr.into(), listen.port)
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
                            task::spawn(listener_task(
                                tentative_socket_id,
                                message_id,
                                socket_addr,
                                self.sender.clone(),
                            ));
                            e.insert(FrontSocketState::Orphan);
                            break;
                        }
                    }
                }
            }

            redshirt_tcp_interface::ffi::TcpMessage::Close(close) => {
                let _ = sockets.remove(&close.socket_id);
            }

            redshirt_tcp_interface::ffi::TcpMessage::Accept(accept) => {
                let message_id = message_id.unwrap(); // TODO: don't unwrap; but what to do?
                sockets
                    .get_mut(&accept.socket_id)
                    .unwrap() // TODO: don't unwrap; but what to do?
                    .as_mut_listener()
                    .unwrap()
                    .send(FrontToBackListener::Accept { message_id })
                    .await
                    .unwrap(); // TODO: don't unwrap; but what to do?
            }

            redshirt_tcp_interface::ffi::TcpMessage::Read(read) => {
                let message_id = message_id.unwrap(); // TODO: don't unwrap; but what to do?
                sockets
                    .get_mut(&read.socket_id)
                    .unwrap() // TODO: don't unwrap; but what to do?
                    .as_mut_connected()
                    .unwrap()
                    .send(FrontToBackSocket::Read { message_id })
                    .await
                    .unwrap(); // TODO: don't unwrap; but what to do?
            }

            redshirt_tcp_interface::ffi::TcpMessage::Write(write) => {
                let message_id = message_id.unwrap(); // TODO: don't unwrap; but what to do?
                sockets
                    .get_mut(&write.socket_id)
                    .unwrap() // TODO: don't unwrap; but what to do?
                    .as_mut_connected()
                    .unwrap()
                    .send(FrontToBackSocket::Write {
                        message_id,
                        data: write.data,
                    })
                    .await
                    .unwrap(); // TODO: don't unwrap; but what to do?
            }
        }
    }

    /// Returns the next message to respond to, and the response.
    pub async fn next_event(&self) -> (TMsgId, Vec<u8>) {
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
                let front_state = sockets.get_mut(&socket_id).unwrap();
                // TODO: debug_assert is orphan
                *front_state = FrontSocketState::Connected(sender);

                (
                    open_message_id,
                    redshirt_tcp_interface::ffi::TcpOpenResponse {
                        result: Ok(socket_id),
                    }
                    .encode(),
                )
            }

            BackToFront::OpenErr {
                open_message_id,
                socket_id,
            } => {
                let mut sockets = self.sockets.lock().await;
                let _front_state = sockets.remove(&socket_id);
                debug_assert!(match _front_state {
                    Some(FrontSocketState::Orphan) => true,
                    _ => false,
                });

                (
                    open_message_id,
                    redshirt_tcp_interface::ffi::TcpOpenResponse { result: Err(()) }.encode(),
                )
            }

            BackToFront::ListenOk {
                listen_message_id,
                local_addr,
                socket_id,
                sender,
            } => {
                let mut sockets = self.sockets.lock().await;
                let front_state = sockets.get_mut(&socket_id).unwrap();
                // TODO: debug_assert is orphan
                *front_state = FrontSocketState::Listener(sender);

                (
                    listen_message_id,
                    redshirt_tcp_interface::ffi::TcpListenResponse {
                        result: Ok((socket_id, local_addr.port())),
                    }
                    .encode(),
                )
            }

            BackToFront::ListenErr {
                listen_message_id,
                socket_id,
            } => {
                let mut sockets = self.sockets.lock().await;
                let _front_state = sockets.remove(&socket_id);
                debug_assert!(match _front_state {
                    Some(FrontSocketState::Orphan) => true,
                    _ => false,
                });

                (
                    listen_message_id,
                    redshirt_tcp_interface::ffi::TcpListenResponse { result: Err(()) }.encode(),
                )
            }

            BackToFront::Accept { message_id, socket } => {
                let mut sockets = self.sockets.lock().await;

                let remote_addr = socket.peer_addr().unwrap(); // TODO: don't unwrap
                let (remote_ip, remote_port) = match remote_addr {
                    SocketAddr::V4(addr) => (addr.ip().to_ipv6_mapped().segments(), addr.port()),
                    SocketAddr::V6(addr) => (addr.ip().segments(), addr.port()),
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
                            let (tx, rx) = mpsc::channel(2);
                            task::spawn(open_socket_task(socket, rx, self.sender.clone()));
                            e.insert(FrontSocketState::Connected(tx));
                            break;
                        }
                    }
                }

                (
                    message_id,
                    redshirt_tcp_interface::ffi::TcpAcceptResponse {
                        accepted_socket_id: tentative_socket_id,
                        remote_ip,
                        remote_port,
                    }
                    .encode(),
                )
            }

            BackToFront::Read { message_id, result } => (
                message_id,
                redshirt_tcp_interface::ffi::TcpReadResponse { result }.encode(),
            ),

            BackToFront::Write { message_id, result } => (
                message_id,
                redshirt_tcp_interface::ffi::TcpWriteResponse { result }.encode(),
            ),
        }
    }
}

impl<TMsgId> Default for TcpState<TMsgId> {
    fn default() -> Self {
        TcpState::new()
    }
}

impl<TMsgId> fmt::Debug for TcpState<TMsgId> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("TcpState").finish()
    }
}

impl<TMsgId> FrontSocketState<TMsgId> {
    fn as_mut_connected(&mut self) -> Option<&mut mpsc::Sender<FrontToBackSocket<TMsgId>>> {
        match self {
            FrontSocketState::Connected(sender) => Some(sender),
            _ => None,
        }
    }

    fn as_mut_listener(&mut self) -> Option<&mut mpsc::Sender<FrontToBackListener<TMsgId>>> {
        match self {
            FrontSocketState::Listener(sender) => Some(sender),
            _ => None,
        }
    }
}

/// Function executed in the background for each TCP socket.
async fn socket_task<TMsgId>(
    socket_id: u32,
    open_message_id: TMsgId,
    socket_addr: SocketAddr,
    mut back_to_front: mpsc::Sender<BackToFront<TMsgId>>,
) {
    // First step is to try connect to the destination.
    let (socket, commands_rx) = match TcpStream::connect(socket_addr).await {
        Ok(s) => {
            let (tx, rx) = mpsc::channel::<FrontToBackSocket<TMsgId>>(2);
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

    open_socket_task(socket, commands_rx, back_to_front).await
}

/// Function executed in the background for each TCP socket.
async fn open_socket_task<TMsgId>(
    mut socket: TcpStream,
    mut commands_rx: mpsc::Receiver<FrontToBackSocket<TMsgId>>,
    mut back_to_front: mpsc::Sender<BackToFront<TMsgId>>,
) {
    // Now that we're connected and we have a `socket` and `commands_rx`, we can start reading
    // and writing.
    loop {
        // TODO: should read and write asynchronously, but that's hard because of borrowing question
        match commands_rx.next().await {
            Some(FrontToBackSocket::Read { message_id }) => {
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
            Some(FrontToBackSocket::Write { message_id, data }) => {
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

/// Function executed in the background for each TCP listener.
async fn listener_task<TMsgId>(
    socket_id: u32,
    listen_message_id: TMsgId,
    socket_addr: SocketAddr,
    mut back_to_front: mpsc::Sender<BackToFront<TMsgId>>,
) {
    // First step is to try create the listener.
    let (listener, mut commands_rx) = match TcpListener::bind(socket_addr).await {
        Ok(s) => {
            let (tx, rx) = mpsc::channel::<FrontToBackListener<TMsgId>>(2);
            let msg_to_front = BackToFront::ListenOk {
                socket_id,
                local_addr: s.local_addr().unwrap(), // TODO:
                listen_message_id,
                sender: tx,
            };

            if back_to_front.send(msg_to_front).await.is_err() {
                return;
            }

            (s, rx)
        }
        Err(_) => {
            let msg_to_front = BackToFront::ListenErr {
                socket_id,
                listen_message_id,
            };
            let _ = back_to_front.send(msg_to_front).await;
            return;
        }
    };

    // Now that we're connected and we have a `listener` and `commands_rx`, we can start reading
    // and writing.
    loop {
        match commands_rx.next().await {
            Some(FrontToBackListener::Accept { message_id }) => {
                let (socket, _) = listener.accept().await.unwrap(); // TODO: don't unwrap
                let msg_to_front = BackToFront::Accept { message_id, socket };
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
