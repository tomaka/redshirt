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

//! Implements the WebGPU interface.

use futures::{channel::mpsc, lock::Mutex, prelude::*, stream::FuturesUnordered};
use redshirt_core::native::{
    DummyMessageIdWrite, NativeProgramEvent, NativeProgramMessageIdWrite, NativeProgramRef,
};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_webgpu_interface::ffi::{WebGPUMessage, INTERFACE};
use std::{
    convert::TryFrom,
    pin::Pin,
    sync::atomic,
    time::{Duration, Instant, SystemTime},
};

/// State machine for `webgpu` interface messages handling.
pub struct WebGPUHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
}

impl WebGPUHandler {
    /// Initializes the new state machine for WebGPU.
    pub fn new() -> Self {
        WebGPUHandler {
            registered: atomic::AtomicBool::new(false),
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a WebGPUHandler {
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

            unimplemented!()
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

        match WebGPUMessage::decode(message) {
            Ok(msg) => {
                /*self.messages_tx
                    .unbounded_send((msg, message_id.unwrap()))
                    .unwrap();*/
            }
            Err(_) => {}
        }
    }

    fn process_destroyed(self, _: Pid) {
        // TODO:
    }

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}
