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

// TODO: this entire code is very work-in-progress

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
    collections::{hash_map::Entry, VecDeque},
    fmt, mem,
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

    /// List of open TCP listeners by port.
    listeners: parking_lot::Mutex<FnvHashMap<u16, mpsc::UnboundedSender<FrontToBackListener>>>,

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
    Close {
        message_id: MessageId,
    },
}

/// Message sent from the main task to the background task for listeners.
enum FrontToBackListener {
    NewSocket {
        socket_id: u32,
        open_message_id: MessageId,
    },
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
    Read {
        message_id: MessageId,
        result: Result<Vec<u8>, redshirt_tcp_interface::ffi::TcpReadError>,
    },
    Write {
        message_id: MessageId,
        result: Result<(), redshirt_tcp_interface::ffi::TcpWriteError>,
    },
    Close {
        message_id: MessageId,
        result: Result<(), redshirt_tcp_interface::ffi::TcpCloseError>,
    },
}

impl TcpHandler {
    /// Initializes a new empty [`TcpHandler`].
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(32);

        TcpHandler {
            registered: atomic::AtomicBool::new(false),
            sockets: parking_lot::Mutex::new(FnvHashMap::default()),
            listeners: parking_lot::Mutex::new(FnvHashMap::default()),
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
                            result: Ok(redshirt_tcp_interface::ffi::TcpSocketOpen {
                                socket_id,
                                local_ip: [0; 8],  // FIXME:
                                local_port: 0,     // FIXME:
                                remote_ip: [0; 8], // FIXME:
                                remote_port: 0,    // FIXME:
                            }),
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

                BackToFront::Close { message_id, result } => {
                    return NativeProgramEvent::Answer {
                        message_id,
                        answer: Ok(redshirt_tcp_interface::ffi::TcpCloseResponse { result }.encode()),
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

                // Find a vacant entry in `self.sockets` with a socket id.
                let vacant_entry = {
                    let mut tentative_socket_id = rand::random();
                    loop {
                        match sockets.entry(tentative_socket_id) {
                            Entry::Vacant(e) => break e,
                            Entry::Occupied(_) => {
                                tentative_socket_id = tentative_socket_id.wrapping_add(1);
                                continue;
                            }
                        }
                    }
                };

                if open.listen {
                    let mut listeners = self.listeners.lock();
                    let listener_sender = listeners
                        .entry(socket_addr.port())
                        .or_insert_with(|| {
                            let (tx, rx) = mpsc::unbounded();
                            // TODO: might not respect the required interface if we have multiple
                            // sockets; we might have to refactor to use REUSE_ADDR and REUSE_PORT
                            // instead
                            task::spawn(listener_task(socket_addr, rx, self.sender.clone()));
                            tx
                        })
                        .clone();
                    listener_sender
                        .unbounded_send(FrontToBackListener::NewSocket {
                            socket_id: *vacant_entry.key(),
                            open_message_id: message_id,
                        })
                        .unwrap();
                    vacant_entry.insert(FrontSocketState::Listener(listener_sender));
                } else {
                    task::spawn(socket_task(
                        *vacant_entry.key(),
                        message_id,
                        socket_addr,
                        self.sender.clone(),
                    ));

                    vacant_entry.insert(FrontSocketState::Orphan);
                }
            }

            ffi::TcpMessage::Close(close) => {
                let message_id = match message_id {
                    Some(m) => m,
                    None => return,
                };

                sockets
                    .get_mut(&close.socket_id)
                    .unwrap() // TODO: don't unwrap; but what to do?
                    .as_mut_connected()
                    .unwrap()
                    .unbounded_send(FrontToBackSocket::Close {
                        message_id,
                    })
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

            ffi::TcpMessage::Destroy(id) => {
                let _ = sockets.remove(&id);
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
    // Buffer of data to write to the TCP socket.
    let mut write_buffer = Vec::new();
    // Value between 0 and `write_buffer.len()` indicating how many bytes at the start of
    // `write_buffer` have already been written to the socket.
    let mut write_buffer_offset = 0;
    // Message to answer when we finish writing the write buffer.
    let mut write_message = None;
    // Buffer where to read data into.
    let mut read_buffer = Vec::new();
    // Message to answer if we read data.
    let mut read_message = None;

    // Now that we're connected and we have a `socket` and `commands_rx`, we can start reading
    // and writing.
    loop {
        enum WhatHappened {
            ReadCmd {
                message_id: MessageId,
            },
            WriteCmd {
                message_id: MessageId,
                data: Vec<u8>,
            },
            CloseCmd {
                message_id: MessageId,
            },
            ReadFinished,
            WriteFinished,
        }

        let what_happened = {
            let partial_write = async {
                if write_message.is_some() {
                    debug_assert!(!write_buffer.is_empty());
                    debug_assert!(write_buffer_offset < write_buffer.len());
                    let num_written = (&socket)
                        .write(&write_buffer[write_buffer_offset..])
                        .await
                        .unwrap(); // TODO: don't unwrap :(
                    debug_assert!(write_buffer_offset + num_written <= write_buffer.len());
                    write_buffer_offset += num_written;
                } else {
                    loop {
                        futures::pending!()
                    }
                }
            };
            futures::pin_mut!(partial_write);
            let read = async {
                if read_message.is_some() {
                    assert!(!read_buffer.is_empty());
                    let num_read = (&socket).read(&mut read_buffer[..]).await.unwrap(); // TODO: don't unwrap :(
                    read_buffer.truncate(num_read);
                } else {
                    loop {
                        futures::pending!()
                    }
                }
            };
            futures::pin_mut!(read);
            let next_command = commands_rx.next();
            futures::pin_mut!(next_command);

            match future::select(future::select(partial_write, read), next_command).await {
                future::Either::Right((Some(FrontToBackSocket::Read { message_id }), _)) => {
                    WhatHappened::ReadCmd { message_id }
                }
                future::Either::Right((Some(FrontToBackSocket::Write { message_id, data }), _)) => {
                    WhatHappened::WriteCmd { message_id, data }
                }
                future::Either::Right((Some(FrontToBackSocket::Close { message_id }), _)) => {
                    WhatHappened::CloseCmd { message_id }
                }
                future::Either::Right((None, _)) => {
                    // `commands_rx` is closed, so let's stop the task.
                    return;
                }
                future::Either::Left((future::Either::Left((_, _)), _)) => {
                    WhatHappened::WriteFinished
                }
                future::Either::Left((future::Either::Right((_, _)), _)) => {
                    WhatHappened::ReadFinished
                }
            }
        };

        match what_happened {
            WhatHappened::ReadCmd { message_id } => {
                // Read already in progress.
                if read_message.is_some() {
                    panic!(); // TODO: don't panic
                }

                assert!(read_buffer.is_empty());
                read_message = Some(message_id);
                read_buffer = vec![0; 512];
            }

            WhatHappened::WriteCmd { message_id, data } => {
                // Write already in progress.
                if write_message.is_some() {
                    panic!(); // TODO: don't panic
                }

                debug_assert!(write_buffer.is_empty());
                debug_assert_eq!(write_buffer_offset, 0);
                write_message = Some(message_id);
                write_buffer = data;
                write_buffer_offset = 0;
            }

            WhatHappened::CloseCmd { message_id } => {
                socket.close();
                let msg_to_front = BackToFront::Close {
                    message_id,
                    result: Ok(()),
                };
                if back_to_front.send(msg_to_front).await.is_err() {
                    return;
                }
            }

            WhatHappened::WriteFinished => {
                // Finished a partial write.
                if write_buffer_offset == write_buffer.len() {
                    let message_id = write_message.take().unwrap();
                    write_buffer.clear();
                    write_buffer_offset = 0;
                    let msg_to_front = BackToFront::Write {
                        message_id,
                        result: Ok(()),
                    };
                    if back_to_front.send(msg_to_front).await.is_err() {
                        return;
                    }
                }
            }

            WhatHappened::ReadFinished => {
                // Finished a read.
                let read_message = read_message.take().unwrap();
                let buf = mem::replace(&mut read_buffer, Vec::new());
                let msg_to_front = BackToFront::Read {
                    message_id: read_message,
                    result: Ok(buf),
                };
                if back_to_front.send(msg_to_front).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Function executed in the background for each TCP listener.
async fn listener_task(
    local_socket_addr: SocketAddr,
    mut front_to_back: mpsc::UnboundedReceiver<FrontToBackListener>,
    mut back_to_front: mpsc::Sender<BackToFront>,
) {
    let socket = match TcpListener::bind(&local_socket_addr).await {
        Ok(socket) => socket,
        Err(_) => return, // TODO: somehow report this
    };

    let mut pending_sockets = VecDeque::new();

    loop {
        enum WhatHappened {
            Cmd(FrontToBackListener),
            NewSocket(TcpStream, SocketAddr),
        }

        let what_happened = {
            let next_command = front_to_back.next();
            futures::pin_mut!(next_command);
            let next_socket = socket.accept();
            futures::pin_mut!(next_socket);

            match future::select(next_command, next_socket).await {
                future::Either::Left((Some(cmd), _)) => WhatHappened::Cmd(cmd),
                future::Either::Left((None, _)) => return,
                future::Either::Right((Ok((socket, addr)), _)) => {
                    WhatHappened::NewSocket(socket, addr)
                }
                future::Either::Right((Err(_), _)) => panic!(), // TODO:
            }
        };

        match what_happened {
            WhatHappened::Cmd(FrontToBackListener::NewSocket {
                socket_id,
                open_message_id,
            }) => {
                pending_sockets.push_back((socket_id, open_message_id));
            }
            WhatHappened::NewSocket(socket, addr) => {
                if let Some((socket_id, open_message_id)) = pending_sockets.pop_front() {
                    let (tx, rx) = mpsc::unbounded();
                    task::spawn(open_socket_task(socket, rx, back_to_front.clone()));

                    let msg_to_front = BackToFront::OpenOk {
                        open_message_id,
                        socket_id,
                        sender: tx,
                    };

                    if back_to_front.send(msg_to_front).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}
