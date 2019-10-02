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

#[cfg(target_arch = "wasm32")] // TODO: bad
pub fn next_message_raw(to_poll: &mut [u64], block: bool) -> Option<Vec<u8>> {
    unsafe {
        let mut out = Vec::with_capacity(32);
        loop {
            let ret = ffi::next_message(
                to_poll.as_mut_ptr(),
                to_poll.len() as u32,
                out.as_mut_ptr(),
                out.capacity() as u32,
                block,
            ) as usize;
            if ret == 0 {
                return None;
            }
            if ret > out.capacity() {
                out.reserve(ret);
                continue;
            }
            out.set_len(ret);
            return Some(out);
        }
    }
}

#[cfg(target_arch = "wasm32")] // TODO: bad
pub fn next_message(to_poll: &mut [u64], block: bool) -> Option<Message> {
    let out = next_message_raw(to_poll, block)?;
    let msg: Message = DecodeAll::decode_all(&out).unwrap();
    Some(msg)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn next_message(to_poll: &mut [u64], block: bool) -> Option<Message> {
    unimplemented!()
}

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

pub async fn emit_message_with_response<T: DecodeAll>(
    interface_hash: [u8; 32],
    msg: impl Encode,
) -> Result<T, ()> {
    let msg_id = emit_message(&interface_hash, &msg, true)?.unwrap();
    Ok(message_response(msg_id).await)
}

pub fn emit_answer(message_id: u64, msg: &impl Encode) -> Result<(), ()> {
    unsafe {
        let buf = msg.encode();
        let ret = ffi::emit_answer(message_id, buf.as_ptr(), buf.len() as u32);
        if ret == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}

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

/// > **WARNING**: Rust (and more importantly LLVM) at the moment assumes that only a single WASM
/// >              thread can exist at any given point in time. More specifically, LLVM assumes
/// >              that only a single stack exists, and maintains a stack pointer as a global
/// >              variable. It is therefore unsound to use stack variables on separate threads.
#[cfg(target_arch = "wasm32")]
pub unsafe fn spawn_thread(function: impl FnOnce()) {
    let function_box: Box<Box<dyn FnOnce()>> = Box::new(Box::new(function));

    extern "C" fn caller(user_data: u32) {
        unsafe {
            let user_data = Box::from_raw(user_data as *mut Box<dyn FnOnce()>);
            user_data();
        }
    }

    let thread_new = threads::ffi::ThreadsMessage::New(threads::ffi::ThreadNew {
        fn_ptr: mem::transmute(caller as extern "C" fn(u32)),
        user_data: Box::into_raw(function_box) as usize as u32,
    });

    emit_message(&threads::ffi::INTERFACE, &thread_new, false).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn spawn_thread(function: impl FnOnce()) {
    panic!()
}

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

#[cfg(not(target_arch = "wasm32"))] // TODO: bad
                                    // TODO: strongly-typed Future
pub fn message_response<T: DecodeAll>(msg_id: u64) -> impl Future<Output = T> {
    panic!();
    future::pending()
}

// TODO: add a variant of message_response but for multiple messages
