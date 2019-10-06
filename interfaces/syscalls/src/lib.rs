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

//! Bindings for interfacing with the environment of the "kernel".
//!
//! # About threads
//!
//! Multithreading in WASM isn't specified yet, and Rust doesn't allow multithreaded WASM code.
//! In particular, multithreaded WASM code in LLVM is undefined behaviour.
//!
//! With that in mind, this makes writing an implementation of `Future` challenging. When the
//! `Future` returns `Poll::Pending`, the `Waker` has to be stored somewhere and invoked. Since
//! there is no possibility of having multiple threads, the only moment when the `Waker` can be
//! invoked is when we explicitly call a function whose role is to do that. The only reasonable
//! choice for such function is the [`block_on`] function, or similar functions.
//! 
//! For the same reason, it is also challenging to write an implementation of [`block_on`].
//! Putting the current thread to sleep is not enough, because the lack of background threads
//! makes it impossible for the `Waker` to be invoked. An implementation of [`block_on`] **must**
//! somehow perform actions that will drive to completion the `Future` it is blocking upon,
//! otherwise nothing will ever happen.
//!
//! Consequently, it has been decided that the implementations of `Future` that this module
//! provide interact, through a global variable, with the behaviour of [`block_on`]. More
//! precisely, before a `Future` returns `Poll::Pending`, it stores its `Waker` in a global
//! variable alongside with the ID of the message whose response ware waiting for, and the
//! [`block_on`] function reads and processes that global variable.
//!
//! It is not possible to build a `Future` that is not built on top of [`MessageResponseFuture`]
//! or [`InterfaceMessageFuture`], and every single use-cases of `Future`s that we could think of
//! can and must be built on top of one of these two `Future`. Similarly, it is not possible to
//! build an implementation of [`block_on`] without having access to the internals of these
//! `Future`s. Tying these `Future`s to the implementation of [`block_on`] is therefore the
//! logical thing to do.
//!

#![deny(intra_doc_link_resolution_failure)]

extern crate alloc;

use alloc::sync::Arc;
use core::{
    hint::unreachable_unchecked,
    marker::PhantomData,
    mem,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use crossbeam::{atomic::AtomicCell, queue::SegQueue};
use futures::prelude::*;
use parity_scale_codec::{DecodeAll, Encode};

pub use block_on::block_on;
pub use ffi::{InterfaceMessage, Message, ResponseMessage};

mod block_on;

pub mod ffi;

/// Emits a message destined to the handler of the given interface.
///
/// Returns `Ok` if the message has been successfully dispatched.
///
/// If `needs_answer` is true, then we expect an answer to the message to come later. A message ID
/// is generated and is returned within `Ok(Some(...))`.
/// If `needs_answer` is false, the function always returns `Ok(None)`.
pub fn emit_message(
    interface_hash: &[u8; 32],
    msg: &impl Encode,
    needs_answer: bool,
) -> Result<Option<u64>, ()> {
    emit_message_raw(interface_hash, &msg.encode(), needs_answer)
}

/// Emits a message destined to the handler of the given interface.
///
/// Returns `Ok` if the message has been successfully dispatched.
///
/// If `needs_answer` is true, then we expect an answer to the message to come later. A message ID
/// is generated and is returned within `Ok(Some(...))`.
/// If `needs_answer` is false, the function always returns `Ok(None)`.
pub fn emit_message_raw(
    interface_hash: &[u8; 32],
    buf: &[u8],
    needs_answer: bool,
) -> Result<Option<u64>, ()> {
    unsafe {
        let mut message_id_out = 0xdeadbeefu64;
        let ret = ffi::emit_message(
            interface_hash as *const [u8; 32] as *const _,
            buf.as_ptr(),
            buf.len() as u32,
            needs_answer,
            &mut message_id_out as *mut u64,
        );
        if ret != 0 {
            return Err(());
        }

        if needs_answer {
            debug_assert_ne!(message_id_out, 0xdeadbeefu64);      // TODO: what if written message_id is actually deadbeef?
            Ok(Some(message_id_out))
        } else {
            Ok(None)
        }
    }
}

/// Combines [`emit_message`] with [`message_response`].
// TODO: the returned Future should have a Drop impl that cancels the message
pub async fn emit_message_with_response<T: DecodeAll>(
    interface_hash: [u8; 32],
    msg: impl Encode,
) -> Result<T, ()> {
    let msg_id = emit_message(&interface_hash, &msg, true)?.unwrap();
    Ok(message_response(msg_id).await)
}

/// Answers the given message.
// TODO: move to interface interface?
pub fn emit_answer(message_id: u64, msg: &impl Encode) -> Result<(), ()> {
    unsafe {
        let buf = msg.encode();
        let ret = ffi::emit_answer(&message_id, buf.as_ptr(), buf.len() as u32);
        if ret == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}

/// Cancel the given message. No answer will be received.
pub fn cancel_message(message_id: u64) -> Result<(), ()> {
    unsafe {
        if ffi::cancel_message(&message_id) == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}

/// Returns a future that is ready when a new message arrives on an interface that we have
/// registered.
///
// TODO: move to interface interface?
pub fn next_interface_message() -> InterfaceMessageFuture {
    InterfaceMessageFuture {
        finished: false,
    }
}

/// Returns a future that is ready when a response to the given message comes back.
///
/// The return value is the type the message decodes to.
pub fn message_response_sync_raw(msg_id: u64) -> Vec<u8> {
    match block_on::next_message(&mut [msg_id], true).unwrap() {
        Message::Response(m) => m.actual_data,
        _ => panic!()
    }
}

/// Returns a future that is ready when a response to the given message comes back.
///
/// The return value is the type the message decodes to.
pub fn message_response<T: DecodeAll>(msg_id: u64) -> MessageResponseFuture<T> {
    MessageResponseFuture {
        finished: false,
        msg_id,
        marker: PhantomData,
    }
}

// TODO: add a variant of message_response but for multiple messages

#[must_use]
pub struct MessageResponseFuture<T> {
    msg_id: u64,
    finished: bool,
    marker: PhantomData<T>,
}

#[cfg(target_arch = "wasm32")] // TODO: bad
impl<T> Future for MessageResponseFuture<T>
where
    T: DecodeAll
{
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        assert!(!self.finished);
        if let Some(message) = block_on::peek_response(self.msg_id) {
            self.finished = true;
            Poll::Ready(DecodeAll::decode_all(&message.actual_data).unwrap())
        } else {
            block_on::register_message_waker(self.msg_id, cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(not(target_arch = "wasm32"))] // TODO: bad
impl<T> Future for MessageResponseFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        panic!()
    }
}

impl<T> Unpin for MessageResponseFuture<T> {
}

#[must_use]
pub struct InterfaceMessageFuture {
    finished: bool,
}

#[cfg(target_arch = "wasm32")] // TODO: bad
impl Future for InterfaceMessageFuture {
    type Output = InterfaceMessage;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        assert!(!self.finished);
        if let Some(message) = block_on::peek_interface_message() {
            self.finished = true;
            Poll::Ready(message)
        } else {
            block_on::register_message_waker(1, cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(not(target_arch = "wasm32"))] // TODO: bad
impl Future for InterfaceMessageFuture {
    type Output = InterfaceMessage;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        panic!()
    }
}

impl Unpin for InterfaceMessageFuture {
}
