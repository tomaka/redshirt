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

use crate::{ffi::DecodedNotification, Decode, EncodedMessage, MessageId};

use core::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use futures::prelude::*;

/// Waits until a response to the given message comes back.
///
/// Returns the undecoded response.
pub fn message_response_sync_raw(msg_id: MessageId) -> EncodedMessage {
    match crate::block_on::next_notification(&mut [msg_id.into()], true).unwrap() {
        DecodedNotification::Response(m) => m.actual_data.unwrap(),
        _ => panic!(),
    }
}

/// Returns a future that is ready when a response to the given message comes back.
///
/// The return value is the type the message decodes to.
pub fn message_response<T: Decode>(msg_id: MessageId) -> MessageResponseFuture<T> {
    MessageResponseFuture {
        finished: false,
        msg_id,
        marker: PhantomData,
    }
}

// TODO: add a variant of message_response but for multiple messages

/// Future that drives `message_response` to completion.
#[must_use]
pub struct MessageResponseFuture<T> {
    msg_id: MessageId,
    finished: bool,
    marker: PhantomData<T>,
}

impl<T> Future for MessageResponseFuture<T>
where
    T: Decode,
{
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        assert!(!self.finished);
        if let Some(response) = crate::block_on::peek_response(self.msg_id) {
            self.finished = true;
            Poll::Ready(Decode::decode(response.actual_data.unwrap()).unwrap())
        } else {
            crate::block_on::register_message_waker(self.msg_id, cx.waker().clone());
            Poll::Pending
        }
    }
}

impl<T> Unpin for MessageResponseFuture<T> {}
