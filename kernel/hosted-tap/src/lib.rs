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

use byteorder::{BigEndian, ByteOrder as _};
use crc::crc32::{self, Hasher32 as _};
use futures::{channel::mpsc, executor::block_on, prelude::*};
use redshirt_core::native::{
    DummyMessageIdWrite, NativeProgramEvent, NativeProgramMessageIdWrite, NativeProgramRef,
};
use redshirt_core::{EncodedMessage, MessageId, Pid, Decode as _, Encode as _};
use redshirt_network_interface::ffi;
use spin::Mutex;
use std::{fmt, io, pin::Pin, sync::Arc, thread};

/// TAP interface that registers itself towards the network manager.
pub struct TapNetworkInterface {
    /// Sender for messages to output on the TAP interface.
    ///
    /// Uses a `Buffer` in order to be able to make sure that sends are going to succeed.
    // TODO: if `mpsc::Sender` gets a `is_ready()` function or something, we can get rid of
    // the `Buffer`
    to_send: Mutex<sink::Buffer<mpsc::Sender<Vec<u8>>, Vec<u8>>>,
    /// Receiver for messages coming from the TAP interface.
    recv: Mutex<mpsc::Receiver<Vec<u8>>>,
    /// If `Some`, the id under which we're registered towards the network manager.
    registered_id: Mutex<Option<u64>>,
    /// If `Some`, we have emitted a message asking for more data to send.
    // TODO: use `AtomicOptionMessageId` or something
    read_message_id: Mutex<Option<MessageId>>,
    /// If `Some`, we have emitted a message injecting data in the interface.
    // TODO: use `AtomicOptionMessageId` or something
    write_message_id: Mutex<Option<MessageId>>,
}

/// Must be used to write back the [`MessageId`] of an emitted message.
#[must_use]
pub struct MessageIdWrite<'a> {
    /// Our parent.
    interface: &'a TapNetworkInterface,
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
                        let mut packet: Vec<u8> = match block_on(to_send_rx.next()) {
                            None => break, // The `TapNetworkInterface` has been dropped.
                            Some(p) => p,
                        };

                        // Append the CRC to the packet.
                        let mut crc_digest = crc32::Digest::new(crc32::IEEE);
                        //let mut crc_digest = crc32::Digest::new_custom(crc32::IEEE, !0u32, !0u32, crc::CalcType::Reverse);
                        crc_digest.write(&packet);
                        let mut crc_bytes = [0; 4];
                        BigEndian::write_u32(&mut crc_bytes, crc_digest.sum32());
                        for b in &mut crc_bytes {
                            *b = b.reverse_bits();
                        }
                        packet.extend_from_slice(&crc_bytes);

                        println!("sending packet: {:?}", packet.iter().map(|b| format!("{:x}", b)).collect::<Vec<_>>().join(" "));

                        /*// Verify our own CRC.
                        debug_assert_eq!(0xc704dd7b, {
                            let mut crc_digest = crc32::Digest::new(crc32::IEEE);
                            crc_digest.write(&packet);
                            crc_digest.sum32()
                        });*/

                        if let Err(err) = interface.send(&packet) {
                            // Error on the tap interface. Killing this thread will close the
                            // channel and thus inform the `TapNetworkInterface` that something
                            // bad happened.
                            println!("error: {:?}", err);
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

                    // TODO: check and discard CRC?

                    println!("rx packet");

                    if block_on(recv_tx.send(buffer)).is_err() {
                        // The `TapNetworkInterface` has been dropped.
                        break;
                    }
                }
            })?;

        Ok(TapNetworkInterface {
            to_send: Mutex::new(to_send.buffer(1)),
            recv: Mutex::new(recv),
            registered_id: Mutex::new(None),
            read_message_id: Mutex::new(None),
            write_message_id: Mutex::new(None),
        })
    }
}

impl<'a> NativeProgramRef<'a> for &'a TapNetworkInterface {
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = MessageIdWrite<'a>;

    fn next_event(self) -> Self::Future {
        Box::pin(async move {
            // Start by registering our device if not done yet.
            let registered_id = {
                let mut registered_id = self.registered_id.try_lock().unwrap();
                match *registered_id {
                    Some(id) => id,
                    None => {
                        let id: u64 = rand::random();
                        let mut mac_address: [u8; 6] = rand::random(); // TODO: ?
                        mac_address[0] &= !0x1; // Ensure the MAC is not multicast.
                        println!("mac = {:?}", mac_address);
                        *registered_id = Some(id);
                        // TODO: communicate MTU to network manager
                        let message = ffi::TcpMessage::RegisterInterface {
                            id,
                            mac_address,
                        };
                        return NativeProgramEvent::Emit {
                            interface: ffi::INTERFACE,
                            message: message.encode(),
                            message_id_write: None,
                        };
                    }
                }
            };

            // Emit, if necessary, a message asking for data to send on the interface.
            let read_message_id = self.read_message_id.try_lock().unwrap();
            if read_message_id.is_none() {
                self.to_send.try_lock().unwrap().flush().await.unwrap();
                return NativeProgramEvent::Emit {
                    interface: ffi::INTERFACE,
                    message: ffi::TcpMessage::InterfaceWaitData(registered_id)
                        .encode(),
                    message_id_write: Some(MessageIdWrite {
                        interface: self,
                        ty: MessageIdWriteTy::Read,
                    }),
                };
            }

            // Emit, if possible, a message feeding data that arrived from the interface.
            let write_message_id = self.write_message_id.try_lock().unwrap();
            if write_message_id.is_none() {
                let data = self.recv.try_lock().unwrap().next().await.unwrap();
                return NativeProgramEvent::Emit {
                    interface: ffi::INTERFACE,
                    message: ffi::TcpMessage::InterfaceOnData(registered_id, data)
                        .encode(),
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
        })
    }

    fn interface_message(self, _: [u8; 32], _: Option<MessageId>, _: Pid, _: EncodedMessage) {
        unreachable!()
    }

    fn process_destroyed(self, _: Pid) {
    }

    fn message_response(self, message_id: MessageId, data: Result<EncodedMessage, ()>) {
        let mut read_message_id = self.read_message_id.try_lock().unwrap();
        let mut write_message_id = self.write_message_id.try_lock().unwrap();

        if Some(message_id) == *read_message_id {
            *read_message_id = None;
            let data = Vec::<u8>::decode(data.unwrap()).unwrap();
            // Sending on `to_send` always succeeds because we make sure that the buffer is empty
            // before emitting a read message.
            self.to_send.try_lock().unwrap().send(data).now_or_never().unwrap().unwrap();
        } else if Some(message_id) == *write_message_id {
            *write_message_id = None;
            debug_assert!(<()>::decode(data.unwrap()).is_ok());
        } else {
            panic!()
        }
    }
}

impl fmt::Debug for TapNetworkInterface {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut w = f.debug_struct("TapNetworkInterface");
        if let Some(registered_id) = &*self.registered_id.try_lock().unwrap() {
            w.field("registered_id", registered_id);
        }
        w.finish()
    }
}

impl<'a> NativeProgramMessageIdWrite for MessageIdWrite<'a> {
    fn acknowledge(self, message_id: MessageId) {
        match self.ty {
            MessageIdWriteTy::Read => {
                let mut read_message_id = self.interface.read_message_id.try_lock().unwrap();
                debug_assert!(read_message_id.is_none());
                *read_message_id = Some(message_id);
            }
            MessageIdWriteTy::Write => {
                let mut write_message_id = self.interface.write_message_id.try_lock().unwrap();
                debug_assert!(write_message_id.is_none());
                *write_message_id = Some(message_id);
            }
        }
    }
}

impl<'a> fmt::Debug for MessageIdWrite<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("MessageIdWrite").finish()
    }
}
