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

use crate::{
    ffi::{DecodedInterfaceOrDestroyed, DecodedNotification},
    Encode, MessageId,
};

use core::{
    convert::TryFrom as _,
    pin::Pin,
    task::{Context, Poll},
};
use futures::prelude::*;

/// Returns a future that is ready when a new message arrives on an interface that we have
/// registered.
// TODO: move to interface interface?
pub fn next_interface_message() -> InterfaceMessageFuture {
    InterfaceMessageFuture {
        finished: false,
        registration: None,
    }
}

/// Answers the given message.
// TODO: move to interface interface?
pub fn emit_answer(message_id: MessageId, msg: impl Encode) {
    #[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
    fn imp(message_id: MessageId, msg: impl Encode) {
        unsafe {
            let buf = msg.encode();
            crate::ffi::emit_answer(&u64::from(message_id), buf.0.as_ptr(), buf.0.len() as u32);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn imp(message_id: MessageId, msg: impl Encode) {
        unreachable!()
    }
    imp(message_id, msg)
}

/// Answers the given message by notifying of an error in the message.
// TODO: move to interface interface?
pub fn emit_message_error(message_id: MessageId) {
    #[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
    fn imp(message_id: MessageId) {
        unsafe { crate::ffi::emit_message_error(&u64::from(message_id)) }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn imp(message_id: MessageId) {
        unreachable!()
    }
    imp(message_id)
}

/// Future that drives [`next_interface_message`] to completion.
#[must_use]
pub struct InterfaceMessageFuture {
    finished: bool,
    registration: Option<crate::block_on::WakerRegistration>,
}

impl Future for InterfaceMessageFuture {
    type Output = DecodedInterfaceOrDestroyed;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        assert!(!self.finished);

        if let Some(message) = crate::block_on::peek_interface_message() {
            self.finished = true;
            return Poll::Ready(message);
        }

        if let Some(r) = &mut self.registration {
            r.update(cx.waker());
            return Poll::Pending;
        }

        // The first time `poll` is called, we normally register the message towards the `block_on`
        // module. But before doing that, we do a peeking syscall to see if a response has already
        // arrived. This makes it possible for code such as `future.now_or_never()` to work.
        if let Some(notif) = crate::block_on::next_notification(&mut [1], false) {
            let msg = match notif {
                DecodedNotification::Interface(msg) => DecodedInterfaceOrDestroyed::Interface(msg),
                DecodedNotification::ProcessDestroyed(msg) => {
                    DecodedInterfaceOrDestroyed::ProcessDestroyed(msg)
                }
                _ => unreachable!(),
            };

            self.finished = true;
            return Poll::Ready(msg);
        }

        self.registration = Some(crate::block_on::register_interface_message_waker(
            cx.waker().clone(),
        ));
        Poll::Pending
    }
}

impl Unpin for InterfaceMessageFuture {}
