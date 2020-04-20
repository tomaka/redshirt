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

use crate::{Decode, Encode, EncodedMessage, InterfaceHash, MessageId};
use core::{
    convert::TryFrom as _,
    fmt,
    marker::PhantomData,
    mem::MaybeUninit,
    num::NonZeroU64,
    pin::Pin,
    task::{Context, Poll},
};
use futures::prelude::*;
use generic_array::{
    sequence::Concat as _,
    typenum::consts::{U0, U2},
    ArrayLength, GenericArray,
};

/// Prototype for a message in construction.
///
/// Use this struct if you want to send out a message split between multiple slices.
pub struct MessageBuilder<'a, TLen: ArrayLength<u32>> {
    /// Parameter for the FFI function.
    allow_delay: bool,
    /// Array of slices, passed to the FFI function.
    array: GenericArray<u32, TLen>,
    /// Pin the lifetime. The lifetime corresponds to the lifetime of buffers pointer to
    /// within `array`.
    marker: PhantomData<&'a ()>,
}

impl<'a> MessageBuilder<'a, U0> {
    /// Start building an empty message.
    pub fn new() -> Self {
        MessageBuilder {
            allow_delay: true,
            array: Default::default(),
            marker: PhantomData,
        }
    }
}

impl<'a, TLen> MessageBuilder<'a, TLen>
where
    TLen: ArrayLength<u32>,
{
    /// If called, emitting the message will fail if no interface handler is available. Otherwise,
    /// emitting the message will block the thread until a handler is available.
    pub fn with_no_delay(mut self) -> Self {
        self.allow_delay = false;
        self
    }

    /// Append a slice of message data to the builder.
    ///
    /// > **Note**: This operation is cheap and doesn't perform any copy of the message data
    /// >           itself.
    pub fn add_data<TOutLen>(self, buffer: &'a EncodedMessage) -> MessageBuilder<'a, TOutLen>
    where
        TLen: core::ops::Add<U2, Output = TOutLen>,
        TOutLen: ArrayLength<u32>,
    {
        self.add_data_raw(&buffer.0)
    }

    /// Append a slice of message data to the builder.
    ///
    /// > **Note**: This operation is cheap and doesn't perform any copy of the message data
    /// >           itself.
    pub fn add_data_raw<TOutLen>(self, buffer: &'a [u8]) -> MessageBuilder<'a, TOutLen>
    where
        TLen: core::ops::Add<U2, Output = TOutLen>,
        TOutLen: ArrayLength<u32>,
    {
        let mut new_pair = GenericArray::<u32, U2>::default();
        new_pair[0] = u32::try_from(buffer.as_ptr() as usize).unwrap();
        new_pair[1] = u32::try_from(buffer.len()).unwrap();

        MessageBuilder {
            allow_delay: self.allow_delay,
            array: self.array.concat(new_pair),
            marker: self.marker,
        }
    }

    /// Emit the message and returns a `Future` that will yield the response.
    // TODO: could we remove the error type?
    pub unsafe fn emit_with_response<T>(
        self,
        interface: &InterfaceHash,
    ) -> Result<impl Future<Output = T>, EmitErr>
    where
        T: Decode,
    {
        let msg_id = self.emit_with_response_raw(interface)?;
        let response_fut = crate::message_response(msg_id);
        Ok(EmitMessageWithResponse {
            inner: Some(response_fut),
            msg_id,
        })
    }

    /// Emit the message and returns the emitted [`MessageId`].
    // TODO: could we remove the error type?
    pub unsafe fn emit_with_response_raw(
        self,
        interface: &InterfaceHash,
    ) -> Result<MessageId, EmitErr> {
        Ok(self.emit_raw(interface, true)?.unwrap())
    }

    /// Emit the message. The message doesn't expect any response. If the handler tries to
    /// respond, the response will be ignored.
    // TODO: could we remove the error type?
    pub unsafe fn emit_without_response(self, interface: &InterfaceHash) -> Result<(), EmitErr> {
        let out = self.emit_raw(interface, false)?;
        debug_assert!(out.is_none());
        Ok(())
    }

    /// Emit the message. You can decide at runtime whether or not the message expects a response.
    ///
    /// If `needs_answer` is `true`, then on success a `Some` will always be returned.
    /// If `needs_answer` is `false`, then on success a `None` will always be returned.
    // TODO: could we remove the error type?
    pub unsafe fn emit_raw(
        self,
        interface: &InterfaceHash,
        needs_answer: bool,
    ) -> Result<Option<MessageId>, EmitErr> {
        self.emit_raw_impl(interface, needs_answer)
    }

    #[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
    unsafe fn emit_raw_impl(
        self,
        interface: &InterfaceHash,
        needs_answer: bool,
    ) -> Result<Option<MessageId>, EmitErr> {
        let mut message_id_out = MaybeUninit::uninit();

        let ret = crate::ffi::emit_message(
            interface as *const InterfaceHash as *const _,
            self.array.as_ptr(),
            u32::try_from(self.array.len() / 2).unwrap(),
            needs_answer,
            self.allow_delay,
            message_id_out.as_mut_ptr(),
        );

        if ret != 0 {
            return Err(EmitErr::BadInterface);
        }

        if needs_answer {
            Ok(Some(MessageId::from(NonZeroU64::new_unchecked(
                message_id_out.assume_init(),
            ))))
        } else {
            Ok(None)
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    unsafe fn emit_raw_impl(
        self,
        _: &InterfaceHash,
        _: bool,
    ) -> Result<Option<MessageId>, EmitErr> {
        unimplemented!()
    }
}

impl<'a> Default for MessageBuilder<'a, U0> {
    fn default() -> Self {
        MessageBuilder::new()
    }
}

impl<'a, TLen> fmt::Debug for MessageBuilder<'a, TLen>
where
    TLen: ArrayLength<u32>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("MessageBuilder").finish()
    }
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
    interface: &InterfaceHash,
    msg: impl Encode,
) -> Result<(), EmitErr> {
    let msg = msg.encode();
    MessageBuilder::new()
        .add_data(&msg)
        .emit_without_response(interface)
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
    interface: &InterfaceHash,
    msg: impl Encode,
) -> Result<impl Future<Output = T>, EmitErr> {
    let msg = msg.encode();
    MessageBuilder::new()
        .add_data(&msg)
        .emit_with_response(interface)
}

/// Cancel the given message. No answer will be received.
///
/// Has no effect if the message is invalid.
pub fn cancel_message(message_id: MessageId) {
    #[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
    fn imp(message_id: MessageId) {
        unsafe { crate::ffi::cancel_message(&u64::from(message_id)) }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn imp(message_id: MessageId) {
        unreachable!()
    }
    imp(message_id)
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
#[pin_project::pin_project]
pub struct EmitMessageWithResponse<T> {
    #[pin]
    inner: Option<crate::MessageResponseFuture<T>>,
    // TODO: redundant with `inner`
    msg_id: MessageId,
}

impl<T: Decode> Future for EmitMessageWithResponse<T> {
    type Output = T;

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
            Poll::Ready(val)
        }
    }
}

/*#[pin_project::pinned_drop]
impl<T> PinnedDrop for EmitMessageWithResponse<T> {
    fn drop(self: Pin<&mut Self>) {
        if self.inner.is_some() {
            let _ = cancel_message(self.msg_id);
        }
    }
}*/
