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

//! Native program that handles the `random` interface.

use futures::{channel::mpsc, lock::Mutex, prelude::*};
use rand::RngCore as _;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_random_interface::ffi::{GenerateResponse, RandomMessage, INTERFACE};
use std::{pin::Pin, sync::atomic};

/// State machine for `random` interface messages handling.
pub struct RandomNativeProgram {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// Message responses waiting to be emitted.
    pending_messages_rx: Mutex<mpsc::UnboundedReceiver<(MessageId, Result<EncodedMessage, ()>)>>,
    /// Sending side of `pending_messages_rx`.
    pending_messages_tx: mpsc::UnboundedSender<(MessageId, Result<EncodedMessage, ()>)>,
}

impl RandomNativeProgram {
    /// Initializes the new state machine for random messages handling.
    pub fn new() -> Self {
        let (pending_messages_tx, pending_messages_rx) = mpsc::unbounded();

        RandomNativeProgram {
            registered: atomic::AtomicBool::new(false),
            pending_messages_tx,
            pending_messages_rx: Mutex::new(pending_messages_rx),
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a RandomNativeProgram {
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        if !self.registered.swap(true, atomic::Ordering::Relaxed) {
            return Box::pin(future::ready(NativeProgramEvent::Emit {
                interface: redshirt_interface_interface::ffi::INTERFACE,
                message_id_write: None,
                message: redshirt_interface_interface::ffi::InterfaceMessage::Register(INTERFACE)
                    .encode(),
            }));
        }

        Box::pin(async move {
            let mut pending_messages_rx = self.pending_messages_rx.lock().await;
            let (message_id, answer) = pending_messages_rx.next().await.unwrap();
            NativeProgramEvent::Answer { message_id, answer }
        })
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        message_id: Option<MessageId>,
        _emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        debug_assert_eq!(interface, INTERFACE);

        let message_id = match message_id {
            Some(m) => m,
            None => return,
        };

        match RandomMessage::decode(message) {
            Ok(RandomMessage::Generate { len }) => {
                let mut out = vec![0; usize::from(len)];
                let mut rng = rand::thread_rng();
                rng.fill_bytes(&mut out);
                let response = GenerateResponse { result: out };
                self.pending_messages_tx
                    .unbounded_send((message_id, Ok(response.encode())))
                    .unwrap();
            }
            Err(_) => self
                .pending_messages_tx
                .unbounded_send((message_id, Err(())))
                .unwrap(),
        }
    }

    fn process_destroyed(self, _: Pid) {}

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}
