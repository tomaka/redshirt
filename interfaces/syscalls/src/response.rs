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

use crate::{Decode, EncodedMessage, MessageId};

use core::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use futures::prelude::*;

/// Waits until a response to the given message comes back.
///
/// Returns the undecoded response.
// TODO: two futures for the same message will compete with each other; document that?
pub fn message_response_sync_raw(msg_id: MessageId) -> EncodedMessage {
    crate::block_on::next_notification(&mut [msg_id.into()], true)
        .unwrap()
        .actual_data
        .unwrap()
}

/// Returns a future that is ready when a response to the given message comes back.
///
/// The return value is the type the message decodes to.
pub fn message_response<T: Decode>(msg_id: MessageId) -> MessageResponseFuture<T> {
    MessageResponseFuture {
        finished: false,
        msg_id,
        registration: None,
        marker: PhantomData,
    }
}

// TODO: add a variant of message_response but for multiple messages

/// Future that drives [`message_response`] to completion.
///
/// This future is "atomic", in the sense that destroying this future will not consume the message
/// unless `Poll::Ready` has been returned. In other words, even if the response would have been
/// ready to be delivered, destroying this future before the response has actually been delivered
/// with `Poll::Ready` does not consume the response.
#[must_use]
pub struct MessageResponseFuture<T> {
    msg_id: MessageId,
    finished: bool,
    registration: Option<crate::block_on::WakerRegistration>,
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
            return Poll::Ready(Decode::decode(response.actual_data.unwrap()).unwrap());
            // TODO: don't unwrap here?
        }

        if let Some(r) = &mut self.registration {
            r.update(cx.waker());
            return Poll::Pending;
        }

        // The first time `poll` is called, we normally register the message towards the `block_on`
        // module. But before doing that, we do a peeking syscall to see if a response has already
        // arrived. This makes it possible for code such as `future.now_or_never()` to work.
        if let Some(notif) = crate::block_on::next_notification(&mut [self.msg_id.into()], false) {
            debug_assert_eq!(notif.index_in_list, 0);
            debug_assert_eq!(notif.message_id, self.msg_id);

            self.finished = true;
            return Poll::Ready(Decode::decode(notif.actual_data.unwrap()).unwrap());
            // TODO: don't unwrap here?
        }

        self.registration = Some(crate::block_on::register_message_waker(
            self.msg_id,
            cx.waker().clone(),
        ));
        Poll::Pending
    }
}

impl<T> Unpin for MessageResponseFuture<T> {}
