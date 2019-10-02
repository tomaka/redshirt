// Copyright(c) 2019 Pierre Krieger

//! Bindings for interfacing with the environment of the "kernel".
//!
//! # About threads
//!
//! Multithreading in WASM isn't specified yet, and Rust doesn't allow multithreaded WASM code.
//! In particular, multithreaded WASM code in LLVM is undefined behaviour.
//!
//! With that in mind, this makes writing an implementation of `Future` challenging. When the
//! `Future` returns `Poll::Pending`, the `Waker` has to be stored somewhere and invoked. Since
//! there is possibility of having multiple threads, the only moment when the `Waker` can be
//! invoked is when we explicitly call a function whose role is to do that. The only reasonable
//! choices for such function are [`block_on`], or similar.
//! 
//! For the same reason, it is also challenging to write an implementation of [`block_on`].
//! Sleeping the current thread is possible, because the lack of background threads makes it
//! impossible for the `Waker` to be invoked. An implementation of [`block_on`] **must** somehow
//! perform actions that will drive to completion the `Future` it is blocking upon, otherwise
//! nothing will ever happen.
//!
//! Consequently, it has been decided that the implementations of `Future` that this module
//! provide interact, through a global variable, with the behaviour of [`block_on`]. More
//! precisely, before a `Future` returns `Poll::Pending`, it stores its `Waker` in a global
//! variable alongside with the ID of the message whose response ware waiting for, and the
//! [`block_on`] function reads and processes that global variable.
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
    unsafe {
        let buf = msg.encode();
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

/// Returns a future that is ready when a new message arrives on an interface that we have
/// registered.
///
// TODO: move to interface interface?
pub fn next_interface_message() -> InterfaceMessageFuture {
    InterfaceMessageFuture {
        cell: Arc::new(AtomicCell::new(None)),
        finished: false,
    }
}

/// Returns a future that is ready when a response to the given message comes back.
///
/// The return value is the type the message decodes to.
pub fn message_response<T: DecodeAll>(msg_id: u64) -> MessageResponseFuture<T> {
    MessageResponseFuture {
        cell: Arc::new(AtomicCell::new(None)),
        finished: false,
        msg_id,
        marker: PhantomData,
    }
}

// TODO: add a variant of message_response but for multiple messages

#[must_use]
pub struct MessageResponseFuture<T> {
    msg_id: u64,
    cell: Arc<AtomicCell<Option<Message>>>,
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
        if let Some(message) = self.cell.take() {
            match message {
                Message::Response(r) => {
                    self.finished = true;
                    Poll::Ready(DecodeAll::decode_all(&r.actual_data).unwrap())
                }
                #[cfg(debug_assertions)]
                _ => unsafe { unreachable_unchecked() },
                #[cfg(not(debug_assertions))]
                _ => unreachable!(),
            }
        } else {
            block_on::register_message_waker(self.msg_id, self.cell.clone(), cx.waker().clone());
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
    cell: Arc<AtomicCell<Option<Message>>>,
    finished: bool,
}

#[cfg(target_arch = "wasm32")] // TODO: bad
impl Future for InterfaceMessageFuture {
    type Output = InterfaceMessage;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        assert!(!self.finished);
        if let Some(message) = self.cell.take() {
            match message {
                Message::Interface(imsg) => {
                    self.finished = true;
                    Poll::Ready(imsg)
                }
                #[cfg(debug_assertions)]
                _ => unsafe { unreachable_unchecked() },
                #[cfg(not(debug_assertions))]
                _ => unreachable!(),
            }
        } else {
            block_on::register_message_waker(1, self.cell.clone(), cx.waker().clone());
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
