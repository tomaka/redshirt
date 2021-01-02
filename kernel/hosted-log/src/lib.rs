// Copyright (C) 2019-2021  Pierre Krieger
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
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, MessageId};
use redshirt_log_interface::ffi::{DecodedLogMessage, Level, INTERFACE};
use std::{borrow::Cow, num::NonZeroU64, pin::Pin};

/// Native program for `log` interface messages handling.
pub struct LogHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::Atomic<bool>,
    /// If `Some`, contains the registration ID towards the `interface` interface.
    registration_id: atomic::Atomic<Option<NonZeroU64>>,
    /// Number of message requests that need to be emitted.
    pending_message_requests: atomic::Atomic<u8>,
    /// If true, enable terminal colors when printing the log messages.
    enable_colors: bool,
}

impl LogHandler {
    /// Initializes the new state machine for logging.
    pub fn new() -> Self {
        LogHandler {
            registered: atomic::Atomic::new(false),
            registration_id: atomic::Atomic::new(None),
            pending_message_requests: atomic::Atomic::new(16),
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

            loop {
                futures::pending!()
            }
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

        match DecodedLogMessage::decode(notification.actual_data) {
            Ok(decoded) => {
                // Remove any control character from log messages, in order to prevent programs
                // from polluting the terminal.
                let message = if decoded.message().chars().any(|c| c.is_control()) {
                    Cow::Owned(
                        decoded
                            .message()
                            .chars()
                            .filter(|c| !c.is_control())
                            .collect(),
                    )
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
                    notification.emitter_pid,
                    level,
                    header_style.suffix(),
                    message
                );
            }
            Err(_) => println!("bad log message from {:?}", notification.emitter_pid),
        }
    }
}
