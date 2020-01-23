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
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_tcp_interface::ffi;
use std::{
    collections::hash_map::Entry,
    fmt,
    net::{Ipv6Addr, SocketAddr},
    pin::Pin,
    sync::atomic,
};

/// Native process for TCP/IP connections that use the host operating system.
pub struct TcpHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,

    /// Receives messages from the sockets background tasks.
    receiver: Mutex<mpsc::Receiver<BackToFront>>,

    /// List of all active sockets. Contains both open and non-open sockets.
    sockets: parking_lot::Mutex<FnvHashMap<u32, FrontSocketState>>,

    /// Sending side of `receiver`. Meant to be cloned and sent to background tasks.
    sender: mpsc::Sender<BackToFront>,
}

/// State of a socket known from the front state.
enum FrontSocketState {
    /// This socket ID is reserved, but the background task is still in the process of opening it.
    Orphan,

    /// The socket is connected. Contains a sender to send commands to the background task.
    Connected(mpsc::UnboundedSender<FrontToBackSocket>),

    /// The socket is a listener.
    Listener(mpsc::UnboundedSender<FrontToBackListener>),
}

/// Message sent from the main task to the background task for sockets.
enum FrontToBackSocket {
    Read {
        message_id: MessageId,
    },
    Write {
        message_id: MessageId,
        data: Vec<u8>,
    },
}

/// Message sent from the main task to the background task for listeners.
enum FrontToBackListener {
    Accept { message_id: MessageId },
}

/// Message sent from a background socket task to the main task.
enum BackToFront {
    OpenOk {
        open_message_id: MessageId,
        socket_id: u32,
        sender: mpsc::UnboundedSender<FrontToBackSocket>,
    },
    OpenErr {
        open_message_id: MessageId,
        socket_id: u32,
    },
    ListenOk {
        listen_message_id: MessageId,
        socket_id: u32,
        local_addr: SocketAddr,
        sender: mpsc::UnboundedSender<FrontToBackListener>,
    },
    ListenErr {
        listen_message_id: MessageId,
        socket_id: u32,
    },
    Read {
        message_id: MessageId,
        result: Result<Vec<u8>, ()>,
    },
    Write {
        message_id: MessageId,
        result: Result<(), ()>,
    },
    Accept {
        message_id: MessageId,
        socket: TcpStream,
    },
}

impl TcpHandler {
    /// Initializes a new empty [`TcpHandler`].
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(32);

        TcpHandler {
            registered: atomic::AtomicBool::new(false),
            sockets: parking_lot::Mutex::new(FnvHashMap::default()),
            receiver: Mutex::new(receiver),
            sender,
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a TcpHandler {
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        Box::pin(async move {
            if !self.registered.swap(true, atomic::Ordering::Relaxed) {
                return NativeProgramEvent::Emit {
                    interface: redshirt_interface_interface::ffi::INTERFACE,
                    message_id_write: None,
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        ffi::INTERFACE,
                    )
                    .encode(),
                };
            }

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
                    let mut sockets = self.sockets.lock();
                    let front_state = sockets.get_mut(&socket_id).unwrap();
                    // TODO: debug_assert is orphan
                    *front_state = FrontSocketState::Connected(sender);

                    return NativeProgramEvent::Answer {
                        message_id: open_message_id,
                        answer: Ok(redshirt_tcp_interface::ffi::TcpOpenResponse {
                            result: Ok(socket_id),
                        }
                        .encode()),
                    };
                }

                BackToFront::OpenErr {
                    open_message_id,
                    socket_id,
                } => {
                    let mut sockets = self.sockets.lock();
                    let _front_state = sockets.remove(&socket_id);
                    debug_assert!(match _front_state {
                        Some(FrontSocketState::Orphan) => true,
                        _ => false,
                    });

                    return NativeProgramEvent::Answer {
                        message_id: open_message_id,
                        answer: Ok(redshirt_tcp_interface::ffi::TcpOpenResponse {
                            result: Err(()),
                        }
                        .encode()),
                    };
                }

                BackToFront::ListenOk {
                    listen_message_id,
                    local_addr,
                    socket_id,
                    sender,
                } => {
                    let mut sockets = self.sockets.lock();
                    let front_state = sockets.get_mut(&socket_id).unwrap();
                    // TODO: debug_assert is orphan
                    *front_state = FrontSocketState::Listener(sender);

                    return NativeProgramEvent::Answer {
                        message_id: listen_message_id,
                        answer: Ok(redshirt_tcp_interface::ffi::TcpListenResponse {
                            result: Ok((socket_id, local_addr.port())),
                        }
                        .encode()),
                    };
                }

                BackToFront::ListenErr {
                    listen_message_id,
                    socket_id,
                } => {
                    let mut sockets = self.sockets.lock();
                    let _front_state = sockets.remove(&socket_id);
                    debug_assert!(match _front_state {
                        Some(FrontSocketState::Orphan) => true,
                        _ => false,
                    });

                    return NativeProgramEvent::Answer {
                        message_id: listen_message_id,
                        answer: Ok(redshirt_tcp_interface::ffi::TcpListenResponse {
                            result: Err(()),
                        }
                        .encode()),
                    };
                }

                BackToFront::Accept { message_id, socket } => {
                    let mut sockets = self.sockets.lock();

                    let remote_addr = socket.peer_addr().unwrap(); // TODO: don't unwrap
                    let (remote_ip, remote_port) = match remote_addr {
                        SocketAddr::V4(addr) => {
                            (addr.ip().to_ipv6_mapped().segments(), addr.port())
                        }
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
                                let (tx, rx) = mpsc::unbounded();
                                task::spawn(open_socket_task(socket, rx, self.sender.clone()));
                                e.insert(FrontSocketState::Connected(tx));
                                break;
                            }
                        }
                    }

                    return NativeProgramEvent::Answer {
                        message_id,
                        answer: Ok(redshirt_tcp_interface::ffi::TcpAcceptResponse {
                            accepted_socket_id: tentative_socket_id,
                            remote_ip,
                            remote_port,
                        }
                        .encode()),
                    };
                }

                BackToFront::Read { message_id, result } => {
                    return NativeProgramEvent::Answer {
                        message_id,
                        answer: Ok(redshirt_tcp_interface::ffi::TcpReadResponse { result }.encode()),
                    }
                }

                BackToFront::Write { message_id, result } => {
                    return NativeProgramEvent::Answer {
                        message_id,
                        answer: Ok(
                            redshirt_tcp_interface::ffi::TcpWriteResponse { result }.encode()
                        ),
                    }
                }
            }
        })
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        message_id: Option<MessageId>,
        _emitter_pid: Pid, // TODO: use to check ownership of sockets
        message: EncodedMessage,
    ) {
        debug_assert_eq!(interface, ffi::INTERFACE);

        let message = match ffi::TcpMessage::decode(message) {
            Ok(msg) => msg,
            Err(_) => return, // TODO: produce error
        };

        let mut sockets = self.sockets.lock();

        match message {
            ffi::TcpMessage::Open(open) => {
                let message_id = match message_id {
                    Some(m) => m,
                    None => return,
                };

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

            ffi::TcpMessage::Listen(listen) => {
                let message_id = match message_id {
                    Some(m) => m,
                    None => return,
                };

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

            ffi::TcpMessage::Close(close) => {
                let _ = sockets.remove(&close.socket_id);
            }

            ffi::TcpMessage::Accept(accept) => {
                let message_id = match message_id {
                    Some(m) => m,
                    None => return,
                };

                sockets
                    .get_mut(&accept.socket_id)
                    .unwrap() // TODO: don't unwrap; but what to do?
                    .as_mut_listener()
                    .unwrap()
                    .unbounded_send(FrontToBackListener::Accept { message_id })
                    .unwrap(); // TODO: don't unwrap; but what to do?
            }

            ffi::TcpMessage::Read(read) => {
                let message_id = match message_id {
                    Some(m) => m,
                    None => return,
                };

                sockets
                    .get_mut(&read.socket_id)
                    .unwrap() // TODO: don't unwrap; but what to do?
                    .as_mut_connected()
                    .unwrap()
                    .unbounded_send(FrontToBackSocket::Read { message_id })
                    .unwrap(); // TODO: don't unwrap; but what to do?
            }

            ffi::TcpMessage::Write(write) => {
                let message_id = match message_id {
                    Some(m) => m,
                    None => return,
                };

                sockets
                    .get_mut(&write.socket_id)
                    .unwrap() // TODO: don't unwrap; but what to do?
                    .as_mut_connected()
                    .unwrap()
                    .unbounded_send(FrontToBackSocket::Write {
                        message_id,
                        data: write.data,
                    })
                    .unwrap(); // TODO: don't unwrap; but what to do?
            }
        }
    }

    fn process_destroyed(self, _: Pid) {
        // TODO: implement
    }

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}

impl Default for TcpHandler {
    fn default() -> Self {
        TcpHandler::new()
    }
}

impl fmt::Debug for TcpHandler {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("TcpHandler").finish()
    }
}

impl FrontSocketState {
    fn as_mut_connected(&mut self) -> Option<&mut mpsc::UnboundedSender<FrontToBackSocket>> {
        match self {
            FrontSocketState::Connected(sender) => Some(sender),
            _ => None,
        }
    }

    fn as_mut_listener(&mut self) -> Option<&mut mpsc::UnboundedSender<FrontToBackListener>> {
        match self {
            FrontSocketState::Listener(sender) => Some(sender),
            _ => None,
        }
    }
}

/// Function executed in the background for each TCP socket.
async fn socket_task(
    socket_id: u32,
    open_message_id: MessageId,
    socket_addr: SocketAddr,
    mut back_to_front: mpsc::Sender<BackToFront>,
) {
    // First step is to try connect to the destination.
    let (socket, commands_rx) = match TcpStream::connect(socket_addr).await {
        Ok(s) => {
            let (tx, rx) = mpsc::unbounded::<FrontToBackSocket>();
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
async fn open_socket_task(
    mut socket: TcpStream,
    mut commands_rx: mpsc::UnboundedReceiver<FrontToBackSocket>,
    mut back_to_front: mpsc::Sender<BackToFront>,
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
async fn listener_task(
    socket_id: u32,
    listen_message_id: MessageId,
    socket_addr: SocketAddr,
    mut back_to_front: mpsc::Sender<BackToFront>,
) {
    // First step is to try create the listener.
    let (listener, mut commands_rx) = match TcpListener::bind(socket_addr).await {
        Ok(s) => {
            let (tx, rx) = mpsc::unbounded::<FrontToBackListener>();
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
