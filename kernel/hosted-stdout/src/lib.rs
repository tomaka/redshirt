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

//! Implements the stdout interface.

use futures::prelude::*;
use parity_scale_codec::{DecodeAll, Encode as _};
use redshirt_core::{MessageId, Pid};
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramRef, NativeProgramEvent};
use redshirt_stdout_interface::ffi::{INTERFACE, StdoutMessage};
use std::{
    io::{self, Write as _},
    pin::Pin,
    sync::atomic,
};

/// Native program for `stdout` interface messages handling.
pub struct StdoutHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
}

impl StdoutHandler {
    /// Initializes the new state machine for stdout.
    pub fn new() -> Self {
        StdoutHandler {
            registered: atomic::AtomicBool::new(false),
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a StdoutHandler {
    type Future = Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        Box::pin(async move {
            if !self.registered.swap(true, atomic::Ordering::Relaxed) {
                return NativeProgramEvent::Emit {
                    interface: redshirt_interface_interface::ffi::INTERFACE,
                    message_id_write: None,
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(INTERFACE).encode(),
                }
            }

            loop {
                futures::pending!()
            }
        })
    }

    fn interface_message(
        self,
        interface: [u8; 32],
        _message_id: Option<MessageId>,
        _emitter_pid: Pid,
        message: Vec<u8>
    ) {
        debug_assert_eq!(interface, INTERFACE);

        match StdoutMessage::decode_all(&message) {
            Ok(StdoutMessage::Message(msg)) => {
                io::stdout().write_all(msg.as_bytes()).unwrap();
            },
            Err(_) => {},
        }
    }

    fn process_destroyed(self, _: Pid) {
    }

    fn message_response(self, _: MessageId, _: Vec<u8>) {
        unreachable!()
    }
}
