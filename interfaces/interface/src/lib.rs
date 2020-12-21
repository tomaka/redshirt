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

//! Interfaces registration.

#![no_std]

extern crate alloc;

use core::num::NonZeroU64;
use futures::prelude::*;
use redshirt_syscalls::{Encode, EncodedMessage, InterfaceHash, MessageId};

pub use ffi::{DecodedInterfaceOrDestroyed, InterfaceRegisterError};

pub mod ffi;

/// Registers the current program as the provider for the given interface hash.
///
/// > **Note**: Interface hashes can be found in the various `ffi` modules of the crates in the
/// >           `interfaces` directory, although that is subject to change.
///
/// Returns an error if there was already a program registered for that interface.
pub async fn register_interface(
    hash: InterfaceHash,
) -> Result<Registration, InterfaceRegisterError> {
    let msg = ffi::InterfaceMessage::Register(hash);
    // Unwrapping is ok because there's always something that handles interface registration.
    let id = {
        let msg: ffi::InterfaceRegisterResponse =
            unsafe { redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg) }
                .unwrap()
                .await;
        msg.result?
    };

    let mut registration = Registration {
        id,
        messages: stream::FuturesOrdered::new(),
    };

    for _ in 0..32 {
        registration.add_message();
    }

    Ok(registration)
}

/// Registered interface.
// TODO: unregister it if dropped? unregistrations aren't supported at the moment
pub struct Registration {
    /// Identifier of the interface registration.
    id: NonZeroU64,
    /// Futures that will resolve when we receive a message on the interface.
    messages: stream::FuturesOrdered<redshirt_syscalls::MessageResponseFuture<EncodedMessage>>,
}

impl Registration {
    /// Returns the next message received on this interface.
    pub async fn next_message_raw(&mut self) -> DecodedInterfaceOrDestroyed {
        let message = self.messages.next().await.unwrap();
        self.add_message();
        ffi::decode_notification(&message.0).unwrap()
    }

    fn add_message(&mut self) {
        self.messages.push(unsafe {
            let message = ffi::InterfaceMessage::NextMessage(self.id).encode();
            let msg_id = redshirt_syscalls::MessageBuilder::new()
                .add_data(&EncodedMessage(message.0))
                .emit_with_response_raw(&ffi::INTERFACE)
                .unwrap();
            redshirt_syscalls::message_response(msg_id)
        });
    }
}

/// Answers the given message.
pub fn emit_answer(message_id: MessageId, msg: impl Encode) {
    #[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
    fn imp(message_id: MessageId, msg: impl Encode) {
        unsafe {
            // TODO: more optimized version ; right now there's extra copies below
            redshirt_syscalls::emit_message_without_response(
                &ffi::INTERFACE,
                ffi::InterfaceMessage::Answer(message_id, Ok(msg.encode().0)),
            ).unwrap()
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn imp(_: MessageId, _: impl Encode) {
        unreachable!()
    }
    imp(message_id, msg)
}

/// Answers the given message by notifying of an error in the message.
pub fn emit_message_error(message_id: MessageId) {
    #[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
    fn imp(message_id: MessageId) {
        unsafe {
            // TODO: more optimized version ; right now there's extra copies below
            redshirt_syscalls::emit_message_without_response(
                &ffi::INTERFACE,
                ffi::InterfaceMessage::Answer(message_id, Err(())),
            ).unwrap()
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn imp(_: MessageId) {
        unreachable!()
    }
    imp(message_id)
}
