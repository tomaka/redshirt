// Copyright(c) 2019 Pierre Krieger

//! Bindings for interfacing with the environment of the "kernel".

#![deny(intra_doc_link_resolution_failure)]

extern crate alloc;

use alloc::sync::Arc;
use core::{
    mem,
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
        let mut event_id_out = 0xdeadbeefu64;
        let ret = ffi::emit_message(
            interface_hash as *const [u8; 32] as *const _,
            buf.as_ptr(),
            buf.len() as u32,
            needs_answer,
            &mut event_id_out as *mut u64,
        );
        if ret != 0 {
            return Err(());
        }

        if needs_answer {
            debug_assert_ne!(event_id_out, 0xdeadbeefu64);      // TODO: what if written event_id is actually deadbeef?
            Ok(Some(event_id_out))
        } else {
            Ok(None)
        }
    }
}

/// Combines [`emit_message`] with [`message_response`].
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
pub fn next_interface_message() -> impl Future<Output = InterfaceMessage> {
    let cell = Arc::new(AtomicCell::new(None));
    let mut finished = false;
    future::poll_fn(move |cx| {
        assert!(!finished);
        if let Some(message) = cell.take() {
            match message {
                Message::Interface(imsg) => {
                    finished = true;
                    Poll::Ready(imsg)
                }
                _ => unreachable!(), // TODO: replace with std::hint::unreachable when we're mature
            }
        } else {
            block_on::register_message_waker(1, cell.clone(), cx.waker().clone());
            Poll::Pending
        }
    })
}

/// Returns a future that is ready when a response to the given message comes back.
///
/// The return value is the type the message decodes to.
#[cfg(target_arch = "wasm32")] // TODO: bad
                               // TODO: strongly-typed Future
                               // TODO: the strongly typed Future should have a Drop impl that cancels the message
pub fn message_response<T: DecodeAll>(msg_id: u64) -> impl Future<Output = T> {
    let cell = Arc::new(AtomicCell::new(None));
    let mut finished = false;
    future::poll_fn(move |cx| {
        assert!(!finished);
        if let Some(message) = cell.take() {
            match message {
                Message::Response(r) => {
                    finished = true;
                    Poll::Ready(DecodeAll::decode_all(&r.actual_data).unwrap())
                }
                _ => unreachable!(), // TODO: replace with std::hint::unreachable when we're mature
            }
        } else {
            block_on::register_message_waker(msg_id, cell.clone(), cx.waker().clone());
            Poll::Pending
        }
    })
}

/// Returns a future that is ready when a response to the given message comes back.
///
/// The return value is the type the message decodes to.
#[cfg(not(target_arch = "wasm32"))] // TODO: bad
                                    // TODO: strongly-typed Future
pub fn message_response<T: DecodeAll>(msg_id: u64) -> impl Future<Output = T> {
    panic!();
    future::pending()
}

// TODO: add a variant of message_response but for multiple messages
