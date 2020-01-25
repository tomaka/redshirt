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

use crate::arch;

use alloc::{boxed::Box, vec::Vec};
use core::{convert::TryFrom as _, pin::Pin, sync::atomic, task::Poll};
use crossbeam_queue::SegQueue;
use futures::{prelude::*, stream::FuturesUnordered};
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_time_interface::ffi::{TimeMessage, INTERFACE};
use spin::Mutex;

/// State machine for `time` interface messages handling.
pub struct TimeHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// List of messages waiting to be emitted with `next_event`.
    // TODO: use futures channels
    pending_messages: SegQueue<(MessageId, Result<EncodedMessage, ()>)>,
    /// List of active timers.
    timers: Mutex<FuturesUnordered<Pin<Box<dyn Future<Output = MessageId> + Send>>>>,
}

impl TimeHandler {
    /// Initializes the new state machine for time accesses.
    pub fn new() -> Self {
        let mut timers = FuturesUnordered::new();
        // We don't want `timers` to ever produce `None`, so we push a dummy futures.
        timers.push(future::pending().boxed());

        TimeHandler {
            registered: atomic::AtomicBool::new(false),
            pending_messages: SegQueue::new(),
            timers: Mutex::new(timers),
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a TimeHandler {
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

        // TODO: wrong; if a message gets pushed, we don't wake up the task
        if let Ok((message_id, answer)) = self.pending_messages.pop() {
            Box::pin(future::ready(NativeProgramEvent::Answer {
                message_id,
                answer,
            }))
        } else {
            Box::pin(future::poll_fn(move |cx| {
                let mut timers = self.timers.lock();
                match Stream::poll_next(Pin::new(&mut *timers), cx) {
                    Poll::Ready(Some(message_id)) => Poll::Ready(NativeProgramEvent::Answer {
                        message_id,
                        answer: Ok(().encode()),
                    }),
                    Poll::Ready(None) => unreachable!(),
                    Poll::Pending => Poll::Pending,
                }
            }))
        }
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
                let now = crate::arch::monotonic_clock();
                self.pending_messages
                    .push((message_id.unwrap(), Ok(now.encode())));
            }
            Ok(TimeMessage::GetSystem) => unimplemented!(),
            Ok(TimeMessage::WaitMonotonic(value)) => {
                let message_id = message_id.unwrap();
                let mut timers = self.timers.lock();
                timers.push(crate::arch::timer(value).map(move |_| message_id).boxed())
            }
            Err(_) => {
                self.pending_messages.push((message_id.unwrap(), Err(())));
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
