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

//! Implements the log interface by printing logs to stdout.

use futures::prelude::*;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_log_interface::ffi::{DecodedLogMessage, Level, INTERFACE};
use std::{borrow::Cow, pin::Pin, sync::atomic};

/// Native program for `log` interface messages handling.
pub struct LogHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// If true, enable terminal colors when printing the log messages.
    enable_colors: bool,
}

impl LogHandler {
    /// Initializes the new state machine for logging.
    pub fn new() -> Self {
        LogHandler {
            registered: atomic::AtomicBool::new(false),
            enable_colors: atty::is(atty::Stream::Stdout),
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a LogHandler {
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

            loop {
                futures::pending!()
            }
        })
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        _: Option<MessageId>,
        emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        debug_assert_eq!(interface, INTERFACE);

        match DecodedLogMessage::decode(message) {
            Ok(decoded) => {
                // Remove any control character from log messages, in order to prevent programs
                // from polluting the terminal.
                let message = if decoded.message().chars().any(|c| c.is_control()) {
                    Cow::Owned(decoded.message().chars().filter(|c| !c.is_control()).collect())
                } else {
                    Cow::Borrowed(decoded.message())
                };
                let mut header_style = ansi_term::Style::default();
                let level = match decoded.level() {
                    Level::Error => "ERR ",
                    Level::Warn => "WARN",
                    Level::Info => "INFO",
                    Level::Debug => "DEBG",
                    Level::Trace => "TRCE",
                };
                if self.enable_colors {
                    header_style.is_dimmed = true;
                }
                println!(
                    "{}[{:?}] [{}]{} {}",
                    header_style.prefix(),
                    emitter_pid,
                    level,
                    header_style.suffix(),
                    message
                );
            }
            Err(_) => println!("bad log message from {:?}", emitter_pid),
        }
    }

    fn process_destroyed(self, _: Pid) {}

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}
