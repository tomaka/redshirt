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

use crate::{ffi::DecodedInterfaceOrDestroyed, Encode, MessageId};

use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures::prelude::*;

/// Returns a future that is ready when a new message arrives on an interface that we have
/// registered.
// TODO: move to interface interface?
pub fn next_interface_message() -> InterfaceMessageFuture {
    InterfaceMessageFuture { finished: false }
}

/// Answers the given message.
// TODO: move to interface interface?
pub fn emit_answer(message_id: MessageId, msg: impl Encode) {
    unsafe {
        let buf = msg.encode();
        crate::ffi::emit_answer(&u64::from(message_id), buf.0.as_ptr(), buf.0.len() as u32);
    }
}

/// Answers the given message by notifying of an error in the message.
// TODO: move to interface interface?
pub fn emit_message_error(message_id: MessageId) {
    unsafe { crate::ffi::emit_message_error(&u64::from(message_id)) }
}

/// Future that drives [`next_interface_message`] to completion.
#[must_use]
pub struct InterfaceMessageFuture {
    finished: bool,
}

impl Future for InterfaceMessageFuture {
    type Output = DecodedInterfaceOrDestroyed;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        assert!(!self.finished);
        if let Some(message) = crate::block_on::peek_interface_message() {
            self.finished = true;
            Poll::Ready(message)
        } else {
            crate::block_on::register_message_waker(From::from(1), cx.waker().clone());
            Poll::Pending
        }
    }
}

impl Unpin for InterfaceMessageFuture {}
