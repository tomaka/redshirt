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

use crate::{arch::PlatformSpecific, future_channel, random::rng::KernelRng};

use alloc::{boxed::Box, sync::Arc, vec};
use core::{pin::Pin, sync::atomic, task::Poll};
use crossbeam_queue::SegQueue;
use futures::prelude::*;
use rand_core::RngCore as _;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_random_interface::ffi::{GenerateResponse, RandomMessage, INTERFACE};

/// State machine for `random` interface messages handling.
pub struct RandomNativeProgram<TPlat> {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// Queue of random number generators. If it is empty, we generate a new one.
    rngs: SegQueue<KernelRng>,
    /// Sending side of `pending_messages`.
    pending_messages_tx: future_channel::UnboundedSender<(MessageId, Result<EncodedMessage, ()>)>,
    /// List of messages waiting to be emitted with `next_event`.
    pending_messages: future_channel::UnboundedReceiver<(MessageId, Result<EncodedMessage, ()>)>,
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<TPlat>>,
}

impl<TPlat> RandomNativeProgram<TPlat> {
    /// Initializes the new state machine for random messages handling.
    pub fn new(platform_specific: Pin<Arc<TPlat>>) -> Self {
        let (pending_messages_tx, pending_messages) = future_channel::channel();
        RandomNativeProgram {
            registered: atomic::AtomicBool::new(false),
            rngs: SegQueue::new(),
            pending_messages_tx,
            pending_messages,
            platform_specific,
        }
    }
}

impl<'a, TPlat> NativeProgramRef<'a> for &'a RandomNativeProgram<TPlat>
where
    TPlat: PlatformSpecific,
{
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
                        INTERFACE,
                    )
                    .encode(),
                };
            }

            future::poll_fn(move |cx| {
                if let Poll::Ready((message_id, answer)) = self.pending_messages.poll_next(cx) {
                    return Poll::Ready(NativeProgramEvent::Answer { message_id, answer });
                }

                Poll::Pending
            })
            .await
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

                let mut rng = if let Some(rng) = self.rngs.pop() {
                    rng
                } else {
                    KernelRng::new(self.platform_specific.clone())
                };

                rng.fill_bytes(&mut out);
                self.rngs.push(rng);
                let response = GenerateResponse { result: out };
                self.pending_messages_tx
                    .unbounded_send((message_id, Ok(response.encode())));
            }
            Err(_) => self
                .pending_messages_tx
                .unbounded_send((message_id, Err(()))),
        }
    }

    fn process_destroyed(self, _: Pid) {}

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}
