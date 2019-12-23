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

//! Registers a network interface that uses
//! [TAP](https://en.wikipedia.org/wiki/TAP_(network_driver)).

// Implementation notes:
//
// Since reading and writing from/to the TAP interface is blocking, we spawn two background threads
// dedicated to these operations.

use futures::{channel::mpsc, executor::block_on, prelude::*};
use redshirt_network_interface::ffi;
use redshirt_syscalls_interface::{Decode as _, Encode as _, MessageId};
use std::{fmt, io, sync::Arc, thread};

/// TAP interface that registers itself towards the network manager.
pub struct TapNetworkInterface {
    /// Sender for messages to output on the TAP interface.
    ///
    /// Uses a `Buffer` in order to be able to make sure that sends are going to succeed.
    // TODO: if `mpsc::Sender` gets a `is_ready()` function or something, we can get rid of
    // the `Buffer`
    to_send: sink::Buffer<mpsc::Sender<Vec<u8>>, Vec<u8>>,
    /// Receiver for messages coming from the TAP interface.
    recv: mpsc::Receiver<Vec<u8>>,
    /// If `Some`, the id under which we're registered towards the network manager.
    registered_id: Option<u64>,
    /// If `Some`, we have emitted a message asking for more data to send.
    read_message_id: Option<MessageId>,
    /// If `Some`, we have emitted a message injecting data in the interface.
    write_message_id: Option<MessageId>,
}

/// Message that the TAP interface requests to be emitted on an interface.
#[must_use]
#[derive(Debug)]
pub struct Message<'a> {
    /// Interface to emit the message on.
    pub interface: [u8; 32],
    /// Encoded message to emit.
    pub message: Vec<u8>,
    /// If `Some`, the message expects a response. The [`MessageIdWrite`] must be used to write
    /// the `MessageId` of the emitted message.
    pub message_id_write: Option<MessageIdWrite<'a>>,
}

/// Must be used to write back the [`MessageId`] of an emitted message.
#[must_use]
pub struct MessageIdWrite<'a> {
    /// Our parent.
    interface: &'a mut TapNetworkInterface,
    /// Where to write the `MessageId` to.
    ty: MessageIdWriteTy,
}

/// Where to write the `MessageId` to.
enum MessageIdWriteTy {
    /// Write in `read_message_id`.
    Read,
    /// Write in `write_message_id`.
    Write,
}

impl TapNetworkInterface {
    /// Initializes a new TAP interface.
    ///
    /// > **Note**: It is extremely common for this method to fail because of lack of
    /// >           priviledges. It might be a good idea to **not** unwrap this `Result`.
    ///
    pub fn new() -> Result<TapNetworkInterface, io::Error> {
        let (to_send, mut to_send_rx) = mpsc::channel(4);
        let (mut recv_tx, recv) = mpsc::channel(4);

        let interface = Arc::new(tun_tap::Iface::new("redshirt-%d", tun_tap::Mode::Tap)?);

        thread::Builder::new()
            .name("tap-sender".to_string())
            .spawn({
                let interface = interface.clone();
                move || {
                    loop {
                        let packet: Vec<u8> = match block_on(to_send_rx.next()) {
                            None => break, // The `TapNetworkInterface` has been dropped.
                            Some(p) => p,
                        };

                        if interface.send(&packet).is_err() {
                            // Error on the tap interface. Killing this thread will close the
                            // channel and thus inform the `TapNetworkInterface` that something
                            // bad happened.
                            break;
                        }
                    }
                }
            })?;

        thread::Builder::new()
            .name("tap-receiver".to_string())
            .spawn(move || {
                let mut read_buffer = vec![0; 1542];

                loop {
                    let buffer = match interface.recv(&mut read_buffer) {
                        Ok(n) => read_buffer[..n].to_owned(),
                        Err(_) => {
                            // Error on the tap interface. Killing this thread will close the
                            // channel and thus inform the `TapNetworkInterface` that something
                            // bad happened.
                            break;
                        }
                    };

                    if block_on(recv_tx.send(buffer)).is_err() {
                        // The `TapNetworkInterface` has been dropped.
                        break;
                    }
                }
            })?;

        Ok(TapNetworkInterface {
            to_send: to_send.buffer(1),
            recv,
            registered_id: None,
            read_message_id: None,
            write_message_id: None,
        })
    }

    /// Fetch the next message to emit.
    pub async fn next_message<'a>(&'a mut self) -> Message<'a> {
        // Start by registering our device if not done yet.
        let registered_id = match self.registered_id {
            Some(id) => id,
            None => {
                let id: u64 = rand::random();
                self.registered_id = Some(id);
                let message = ffi::TcpMessage::RegisterInterface {
                    id,
                    mac_address: rand::random(), // TODO: ?
                };
                return Message {
                    interface: ffi::INTERFACE,
                    message: message.encode().into_owned(),
                    message_id_write: None,
                };
            }
        };

        // Emit, if necessary, a message asking for data to send on the interface.
        if self.read_message_id.is_none() {
            self.to_send.flush().await.unwrap();
            return Message {
                interface: ffi::INTERFACE,
                message: ffi::TcpMessage::InterfaceWaitData(registered_id)
                    .encode()
                    .into_owned(),
                message_id_write: Some(MessageIdWrite {
                    interface: self,
                    ty: MessageIdWriteTy::Read,
                }),
            };
        }

        // Emit, if possible, a message feeding data that arrived from the interface.
        if self.write_message_id.is_none() {
            let data = self.recv.next().await.unwrap();
            return Message {
                interface: ffi::INTERFACE,
                message: ffi::TcpMessage::InterfaceOnData(registered_id, data)
                    .encode()
                    .into_owned(),
                message_id_write: Some(MessageIdWrite {
                    interface: self,
                    ty: MessageIdWriteTy::Write,
                }),
            };
        }

        // If we reach here, there's nothing we can do, and the user is expected to call
        // `message_answer` to make progress.
        loop {
            futures::pending!()
        }
    }

    /// Call when a message emitted using [`next_message`] gets an answer.
    pub fn message_answer(&mut self, message_id: MessageId, data: Vec<u8>) {
        if Some(message_id) == self.read_message_id {
            self.read_message_id = None;
            let data = Vec::<u8>::decode(data).unwrap();
            // Sending on `to_send` always succeeds because we make sure that the buffer is empty
            // before emitting a read message.
            self.to_send.send(data).now_or_never().unwrap().unwrap();
        } else if Some(message_id) == self.write_message_id {
            self.write_message_id = None;
            debug_assert!(<()>::decode(data).is_ok());
        } else {
            panic!()
        }
    }
}

impl fmt::Debug for TapNetworkInterface {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut w = f.debug_struct("TapNetworkInterface");
        if let Some(registered_id) = &self.registered_id {
            w.field("registered_id", registered_id);
        }
        w.finish()
    }
}

impl<'a> MessageIdWrite<'a> {
    /// Notify that the message has been emitted, and indicates the corresponding [`MessageId`].
    pub fn acknowledge(self, message_id: MessageId) {
        match self.ty {
            MessageIdWriteTy::Read => {
                debug_assert!(self.interface.read_message_id.is_none());
                self.interface.read_message_id = Some(message_id);
            }
            MessageIdWriteTy::Write => {
                debug_assert!(self.interface.write_message_id.is_none());
                self.interface.write_message_id = Some(message_id);
            }
        }
    }
}

impl<'a> fmt::Debug for MessageIdWrite<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("MessageIdWrite").finish()
    }
}
