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

use crate::{ffi::InterfaceOrDestroyed, Encode, MessageId};

use core::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};
use futures::prelude::*;

// TODO: replace `InterfaceOrDestroyed` with a different enum where `actual_data` is more strongly typed

/// Returns a future that is ready when a new message arrives on an interface that we have
/// registered.
// TODO: move to interface interface?
pub fn next_interface_message() -> InterfaceMessageFuture {
    InterfaceMessageFuture { finished: false }
}

/// Answers the given message.
// TODO: move to interface interface?
pub fn emit_answer(message_id: MessageId, msg: impl Encode) -> Result<(), EmitAnswerErr> {
    unsafe {
        let buf = msg.encode();
        let ret =
            crate::ffi::emit_answer(&u64::from(message_id), buf.0.as_ptr(), buf.0.len() as u32);
        if ret == 0 {
            Ok(())
        } else {
            Err(EmitAnswerErr::InvalidMessageId)
        }
    }
}

/// Answers the given message by notifying of an error in the message.
// TODO: move to interface interface?
pub fn emit_message_error(message_id: MessageId) -> Result<(), EmitAnswerErr> {
    unsafe {
        if crate::ffi::emit_message_error(&u64::from(message_id)) == 0 {
            Ok(())
        } else {
            Err(EmitAnswerErr::InvalidMessageId)
        }
    }
}

/// Error that can be retuend by [`emit_answer`].
#[derive(Debug)]
pub enum EmitAnswerErr {
    /// The message ID is not valid or has already been answered.
    InvalidMessageId,
}

impl fmt::Display for EmitAnswerErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EmitAnswerErr::InvalidMessageId => write!(f, "Invalid message ID"),
        }
    }
}

/// Future that drives [`next_interface_message`] to completion.
#[must_use]
pub struct InterfaceMessageFuture {
    finished: bool,
}

impl Future for InterfaceMessageFuture {
    type Output = InterfaceOrDestroyed;

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
