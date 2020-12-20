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

//! Native program that handles the `kernel_log` interface.

use crate::{arch::PlatformSpecific, future_channel};

use alloc::{boxed::Box, sync::Arc};
use core::{num::NonZeroU64, pin::Pin, str, task::Poll};
use futures::prelude::*;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId};
use redshirt_kernel_log_interface::ffi::{KernelLogMethod, INTERFACE};

/// State machine for `random` interface messages handling.
pub struct KernelLogNativeProgram<TPlat> {
    /// If true, we have sent the interface registration message.
    registered: atomic::Atomic<bool>,
    /// If `Some`, contains the registration ID towards the `interface` interface.
    registration_id: atomic::Atomic<Option<NonZeroU64>>,
    /// Number of message requests that need to be emitted.
    pending_message_requests: atomic::Atomic<u8>,
    /// Sending side of `pending_messages`.
    pending_messages_tx: future_channel::UnboundedSender<(MessageId, Result<EncodedMessage, ()>)>,
    /// List of messages waiting to be emitted with `next_event`.
    pending_messages: future_channel::UnboundedReceiver<(MessageId, Result<EncodedMessage, ()>)>,
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<TPlat>>,
}

impl<TPlat> KernelLogNativeProgram<TPlat> {
    /// Initializes the native program.
    pub fn new(platform_specific: Pin<Arc<TPlat>>) -> Self {
        let (pending_messages_tx, pending_messages) = future_channel::channel();
        KernelLogNativeProgram {
            registered: atomic::Atomic::new(false),
            registration_id: atomic::Atomic::new(None),
            pending_message_requests: atomic::Atomic::new(16),
            pending_messages_tx,
            pending_messages,
            platform_specific,
        }
    }
}

impl<'a, TPlat> NativeProgramRef<'a> for &'a KernelLogNativeProgram<TPlat>
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
                    return Poll::Ready(NativeProgramEvent::Answer { message_id, answer });
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

        match notification.actual_data.0.get(0) {
            Some(0) => {
                // Log message.
                let message = &notification.actual_data.0[1..];
                if message.is_ascii() {
                    self.platform_specific
                        .write_log(str::from_utf8(message).unwrap());
                }
            }
            Some(1) => {
                // New log method.
                unimplemented!(); // TODO:
                                  /*if let Ok(method) = KernelLogMethod::decode(&notification.actual_data.0[1..]) {
                                      self.klogger.set_method(method);
                                      if let Some(message_id) = notification.message_id {
                                          self.pending_messages_tx.unbounded_send((message_id, Ok(().encode())))
                                      }
                                  } else {
                                      if let Some(message_id) = notification.message_id {
                                          self.pending_messages_tx.unbounded_send((message_id, Err(())))
                                      }
                                  }*/
            }
            _ => {
                if let Some(message_id) = notification.message_id {
                    self.pending_messages_tx
                        .unbounded_send((message_id, Err(())))
                }
            }
        }
    }
}
