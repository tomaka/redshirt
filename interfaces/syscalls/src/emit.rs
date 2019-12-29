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

use crate::{Decode, Encode, InterfaceHash, MessageId};
use core::{
    fmt,
    mem::MaybeUninit,
    pin::Pin,
    task::{Context, Poll},
};
use futures::prelude::*;

/// Emits a message destined to the handler of the given interface.
///
/// Returns `Ok` if the message has been successfully dispatched. Returns an error if no handler
/// is available for that interface.
/// Whether this function succeeds only depends on whether an interface handler is available. This
/// function doesn't perform any validity check on the message itself.
///
/// If `needs_answer` is true, then we expect an answer to the message to come later. A message ID
/// is generated and is returned within `Ok(Some(...))`.
/// If `needs_answer` is false, the function always returns `Ok(None)`.
///
/// # Safety
///
/// While the action of sending a message is totally safe, the message itself might instruct the
/// environment to perform actions that would lead to unsafety.
///
pub unsafe fn emit_message<'a>(
    interface_hash: &InterfaceHash,
    msg: impl Encode,
    needs_answer: bool,
) -> Result<Option<MessageId>, EmitErr> {
    let encoded = msg.encode();
    emit_message_raw(interface_hash, &encoded.0, needs_answer).map(|r| r.map(MessageId::from))
}

/// Emits a message destined to the handler of the given interface.
///
/// Returns `Ok` if the message has been successfully dispatched. Returns an error if no handler
/// is available for that interface.
/// Whether this function succeeds only depends on whether an interface handler is available. This
/// function doesn't perform any validity check on the message itself.
///
/// # Safety
///
/// While the action of sending a message is totally safe, the message itself might instruct the
/// environment to perform actions that would lead to unsafety.
///
pub unsafe fn emit_message_without_response<'a>(
    interface_hash: &InterfaceHash,
    msg: impl Encode,
) -> Result<(), EmitErr> {
    emit_message(interface_hash, msg, false)?;
    Ok(())
}

/// Emits a message destined to the handler of the given interface.
///
/// Returns `Ok` if the message has been successfully dispatched. Returns an error if no handler
/// is available for that interface.
/// Whether this function succeeds only depends on whether an interface handler is available. This
/// function doesn't perform any validity check on the message itself.
///
/// If `needs_answer` is true, then we expect an answer to the message to come later. A message ID
/// is generated and is returned within `Ok(Some(...))`.
/// If `needs_answer` is false, the function always returns `Ok(None)`.
///
/// # Safety
///
/// While the action of sending a message is totally safe, the message itself might instruct the
/// environment to perform actions that would lead to unsafety.
///
pub unsafe fn emit_message_raw(
    interface_hash: &InterfaceHash,
    buf: &[u8],
    needs_answer: bool,
) -> Result<Option<MessageId>, EmitErr> {
    let mut message_id_out = MaybeUninit::uninit();

    let ret = crate::ffi::emit_message(
        interface_hash as *const InterfaceHash as *const _,
        buf.as_ptr(),
        buf.len() as u32,
        needs_answer,
        true,
        message_id_out.as_mut_ptr(),
    );

    if ret != 0 {
        return Err(EmitErr::BadInterface);
    }

    if needs_answer {
        Ok(Some(MessageId::from(message_id_out.assume_init())))
    } else {
        Ok(None)
    }
}

/// Emis a message, then waits for a response to come back.
///
/// Returns `Ok` if the message has been successfully dispatched. Returns an error if no handler
/// is available for that interface.
/// Whether this function succeeds only depends on whether an interface handler is available. This
/// function doesn't perform any validity check on the message itself.
///
/// The returned future will cancel the message if it is dropped early.
///
/// # Safety
///
/// While the action of sending a message is totally safe, the message itself might instruct the
/// environment to perform actions that would lead to unsafety.
///
pub unsafe fn emit_message_with_response<'a, T: Decode>(
    interface_hash: InterfaceHash,
    msg: impl Encode,
) -> impl Future<Output = Result<T, EmitErr>> {
    let msg_id = match emit_message(&interface_hash, msg, true) {
        Ok(m) => m.unwrap(),
        Err(err) => return future::Either::Right(future::ready(Err(err))),
    };
    let response_fut = crate::message_response(msg_id);
    future::Either::Left(EmitMessageWithResponse {
        inner: Some(response_fut),
        msg_id,
    })
}

/// Cancel the given message. No answer will be received.
pub fn cancel_message(message_id: MessageId) {
    unsafe { crate::ffi::cancel_message(&u64::from(message_id)) }
}

/// Error that can be retuend by functions that emit a message.
#[derive(Debug)]
pub enum EmitErr {
    /// The given interface has no handler.
    BadInterface,
}

impl fmt::Display for EmitErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EmitErr::BadInterface => write!(f, "The given interface has no handler"),
        }
    }
}

/// Future that drives [`emit_message_with_response`] to completion.
#[must_use]
#[pin_project::pin_project(PinnedDrop)]
pub struct EmitMessageWithResponse<T> {
    #[pin]
    inner: Option<crate::MessageResponseFuture<T>>,
    // TODO: redundant with `inner`
    msg_id: MessageId,
}

impl<T: Decode> Future for EmitMessageWithResponse<T> {
    type Output = Result<T, EmitErr>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        unsafe {
            let mut this = self.project();
            let val = match this
                .inner
                .as_mut()
                .map_unchecked_mut(|opt| opt.as_mut().unwrap())
                .poll(cx)
            {
                Poll::Ready(val) => val,
                Poll::Pending => return Poll::Pending,
            };
            *this.inner = None;
            Poll::Ready(Ok(val))
        }
    }
}

#[pin_project::pinned_drop]
impl<T> PinnedDrop for EmitMessageWithResponse<T> {
    fn drop(self: Pin<&mut Self>) {
        if self.inner.is_some() {
            let _ = cancel_message(self.msg_id);
        }
    }
}
