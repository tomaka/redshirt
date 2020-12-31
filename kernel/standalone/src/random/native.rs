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
use core::{num::NonZeroU64, pin::Pin, task::Poll};
use crossbeam_queue::SegQueue;
use futures::prelude::*;
use rand_core::RngCore as _;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, MessageId};
use redshirt_random_interface::ffi::{GenerateResponse, RandomMessage, INTERFACE};

/// State machine for `random` interface messages handling.
pub struct RandomNativeProgram {
    /// If true, we have sent the interface registration message.
    registered: atomic::Atomic<bool>,
    /// If `Some`, contains the registration ID towards the `interface` interface.
    registration_id: atomic::Atomic<Option<NonZeroU64>>,
    /// Number of message requests that need to be emitted.
    pending_message_requests: atomic::Atomic<u8>,
    /// Queue of random number generators. If it is empty, we generate a new one.
    rngs: SegQueue<KernelRng>,
    /// Sending side of `pending_messages`.
    pending_messages_tx: future_channel::UnboundedSender<(MessageId, Result<EncodedMessage, ()>)>,
    /// List of messages waiting to be emitted with `next_event`.
    pending_messages: future_channel::UnboundedReceiver<(MessageId, Result<EncodedMessage, ()>)>,
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<PlatformSpecific>>,
}

impl RandomNativeProgram {
    /// Initializes the new state machine for random messages handling.
    pub fn new(platform_specific: Pin<Arc<PlatformSpecific>>) -> Self {
        let (pending_messages_tx, pending_messages) = future_channel::channel();
        RandomNativeProgram {
            registered: atomic::Atomic::new(false),
            registration_id: atomic::Atomic::new(None),
            pending_message_requests: atomic::Atomic::new(16),
            rngs: SegQueue::new(),
            pending_messages_tx,
            pending_messages,
            platform_specific,
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a RandomNativeProgram {
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        Box::pin(async move {
            if !self.registered.swap(true, atomic::Ordering::Relaxed) {
                return NativeProgramEvent::Emit {
                    interface: redshirt_interface_interface::ffi::INTERFACE,
                    message_id_write: Some(DummyMessageIdWrite),
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        INTERFACE,
                    )
                    .encode(),
                };
            }

            if let Some(registration_id) = self.registration_id.load(atomic::Ordering::Relaxed) {
                loop {
                    let v = self
                        .pending_message_requests
                        .load(atomic::Ordering::Relaxed);
                    if v == 0 {
                        break;
                    }
                    if self
                        .pending_message_requests
                        .compare_exchange(
                            v,
                            v - 1,
                            atomic::Ordering::Relaxed,
                            atomic::Ordering::Relaxed,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    return NativeProgramEvent::Emit {
                        interface: redshirt_interface_interface::ffi::INTERFACE,
                        message_id_write: Some(DummyMessageIdWrite),
                        message: redshirt_interface_interface::ffi::InterfaceMessage::NextMessage(
                            registration_id,
                        )
                        .encode(),
                    };
                }
            }

            future::poll_fn(move |cx| {
                if let Poll::Ready((message_id, answer)) = self.pending_messages.poll_next(cx) {
                    return Poll::Ready(NativeProgramEvent::Emit {
                        interface: redshirt_interface_interface::ffi::INTERFACE,
                        message_id_write: None,
                        message: redshirt_interface_interface::ffi::InterfaceMessage::Answer(
                            message_id,
                            answer.map(|m| m.0),
                        )
                        .encode(),
                    });
                }

                Poll::Pending
            })
            .await
        })
    }

    fn message_response(self, _: MessageId, response: Result<EncodedMessage, ()>) {
        debug_assert!(self.registered.load(atomic::Ordering::Relaxed));

        // The first ever message response that can be received is the interface registration.
        if self
            .registration_id
            .load(atomic::Ordering::Relaxed)
            .is_none()
        {
            let registration_id =
                match redshirt_interface_interface::ffi::InterfaceRegisterResponse::decode(
                    response.unwrap(),
                )
                .unwrap()
                .result
                {
                    Ok(id) => id,
                    // A registration error means the interface has already been registered. Returning
                    // here stalls this state machine forever.
                    Err(_) => return,
                };

            self.registration_id
                .store(Some(registration_id), atomic::Ordering::Relaxed);
            return;
        }

        // If this is reached, the response is a response to a message request.
        self.pending_message_requests
            .fetch_add(1, atomic::Ordering::Relaxed);

        let notification =
            match redshirt_interface_interface::ffi::decode_notification(&response.unwrap().0)
                .unwrap()
            {
                redshirt_interface_interface::DecodedInterfaceOrDestroyed::Interface(n) => n,
                _ => return,
            };

        let message_id = match notification.message_id {
            Some(m) => m,
            None => return,
        };

        match RandomMessage::decode(notification.actual_data) {
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
}
