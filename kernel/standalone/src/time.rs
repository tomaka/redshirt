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

//! Implements the `time` interface.

use crate::{arch::PlatformSpecific, future_channel};

use alloc::{boxed::Box, sync::Arc};
use core::{pin::Pin, sync::atomic, task::Poll};
use crossbeam_queue::SegQueue;
use futures::{prelude::*, stream::FuturesUnordered};
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_time_interface::ffi::{TimeMessage, INTERFACE};
use spinning_top::Spinlock;

/// State machine for `time` interface messages handling.
pub struct TimeHandler<TPlat> {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<TPlat>>,
    /// Sending side of `pending_messages`.
    pending_messages_tx:
        future_channel::UnboundedSender<Option<(MessageId, Result<EncodedMessage, ()>)>>,
    /// List of messages waiting to be emitted with `next_event`. Can also contain dummy events
    /// (`None`) if we just need to wake up the receiving task after having pushed an element on
    /// `timers`.
    pending_messages:
        future_channel::UnboundedReceiver<Option<(MessageId, Result<EncodedMessage, ()>)>>,
    /// List of active timers.
    timers: Spinlock<FuturesUnordered<Pin<Box<dyn Future<Output = MessageId> + Send>>>>,
}

impl<TPlat> TimeHandler<TPlat> {
    /// Initializes the new state machine for time accesses.
    pub fn new(platform_specific: Pin<Arc<TPlat>>) -> Self {
        let (pending_messages_tx, pending_messages) = future_channel::channel();

        TimeHandler {
            registered: atomic::AtomicBool::new(false),
            platform_specific,
            pending_messages_tx,
            pending_messages,
            timers: Spinlock::new(FuturesUnordered::new()),
        }
    }
}

impl<'a, TPlat> NativeProgramRef<'a> for &'a TimeHandler<TPlat>
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
                while let Poll::Ready(msg) = self.pending_messages.poll_next(cx) {
                    if let Some((message_id, answer)) = msg {
                        return Poll::Ready(NativeProgramEvent::Answer { message_id, answer });
                    }
                }

                let mut timers = self.timers.lock();
                if !timers.is_empty() {
                    match Stream::poll_next(Pin::new(&mut *timers), cx) {
                        Poll::Ready(Some(message_id)) => Poll::Ready(NativeProgramEvent::Answer {
                            message_id,
                            answer: Ok(().encode()),
                        }),
                        Poll::Ready(None) => unreachable!(),
                        Poll::Pending => Poll::Pending,
                    }
                } else {
                    Poll::Pending
                }
            })
            .await
        })
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        debug_assert_eq!(interface, INTERFACE);

        match TimeMessage::decode(message) {
            Ok(TimeMessage::GetMonotonic) => {
                let now = self.platform_specific.as_ref().monotonic_clock();
                self.pending_messages_tx
                    .unbounded_send(Some((message_id.unwrap(), Ok(now.encode()))));
            }
            Ok(TimeMessage::WaitMonotonic(value)) => {
                let message_id = message_id.unwrap();
                let timers = self.timers.lock();
                timers.push(
                    self.platform_specific
                        .as_ref()
                        .timer(value)
                        .map(move |_| message_id)
                        .boxed(),
                );
                self.pending_messages_tx.unbounded_send(None);
            }
            Err(_) => {
                self.pending_messages_tx
                    .unbounded_send(Some((message_id.unwrap(), Err(()))));
            }
        }
    }

    fn process_destroyed(self, _: Pid) {
        // TODO:
    }

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}
