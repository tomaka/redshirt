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

//! As explained in the crate root, the futures of this crate can only work with [`block_on`], and
//! vice-versa.
//!
//! The way it works is the following:
//!
//! - We hold a global buffer of interface messages waiting to be processed, and a global buffer
//!   of responses that have been received and that are waiting to be processed.
//!
//! - We also hold a global buffer of message IDs that we want a response for, and an associated
//!   `core::task::Waker`.
//!
//! - When one of the `Future`s gets polled, it first look whether an interface message or a
//!   response is available in one of the buffers. If not, it registers a waker using the
//!   [`register_message_waker`] function.
//!
//! - The [`block_on`] function polls the `Future` passed to it, which optionally calls
//!   [`register_message_waker`], then asks the kernel for responses to the message IDs that have
//!   been registered. Once one or more messages have come back, we poll the `Future` again.
//!   Repeat until the `Future` has ended.
//!

use crate::{
    ffi, DecodedInterfaceOrDestroyed, DecodedNotification, DecodedResponseNotification, MessageId,
};
use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use core::{
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};
use futures::{prelude::*, task};
use hashbrown::HashMap;
use nohash_hasher::BuildNoHashHasher;
use slab::Slab;
use spin::Mutex;

/// Registers a message ID (or 1 for interface messages) and a waker. The `block_on` function will
/// then ask the kernel for a message corresponding to this ID. If one is received, the `Waker`
/// is called.
///
/// For non-interface messages, there can only ever be one registered `Waker`. Registering a
/// `Waker` a second time overrides the one previously registered.
pub(crate) fn register_message_waker(message_id: MessageId, waker: Waker) -> WakerRegistration {
    let mut state = (&*STATE).lock();

    let index = state.wakers.insert(Some(waker));

    if index <= state.message_ids.len() {
        state.message_ids.resize(index + 1, 0);
    }

    debug_assert_eq!(state.message_ids[index], 0);
    state.message_ids[index] = From::from(message_id);

    WakerRegistration { index }
}

/// Removes one element from the global buffer of interface messages waiting to be processed.
pub(crate) fn peek_interface_message() -> Option<DecodedInterfaceOrDestroyed> {
    let mut state = (&*STATE).lock();
    state.interface_messages_queue.pop_front()
}

/// If a response to this message ID has previously been obtained, extracts it for processing.
pub(crate) fn peek_response(msg_id: MessageId) -> Option<DecodedResponseNotification> {
    let mut state = (&*STATE).lock();
    state.pending_messages.remove(&msg_id)
}

pub(crate) struct WakerRegistration {
    /// Index within `STATE::message_ids` and `STATE::wakers`.
    index: usize,
}

impl WakerRegistration {
    /// Modifies the registered waker.
    pub fn update(&self, waker: &Waker) {
        let mut state = (&*STATE).lock();
        match &mut state.wakers[self.index] {
            Some(w) if w.will_wake(waker) => {}
            w @ _ => *w = Some(waker.clone()),
        }
    }
}

impl Drop for WakerRegistration {
    fn drop(&mut self) {
        let mut state = (&*STATE).lock();
        state.message_ids.remove(self.index);
        state.wakers.remove(self.index);
    }
}

/// Blocks the current thread until the [`Future`](core::future::Future) passed as parameter
/// finishes.
pub fn block_on<T>(future: impl Future<Output = T>) -> T {
    futures::pin_mut!(future);

    // This `Arc<AtomicBool>` will be set to true if we are waken up during the polling.
    let woken_up = Arc::new(AtomicBool::new(false));
    let waker = {
        struct Notify(Arc<AtomicBool>);
        impl task::ArcWake for Notify {
            fn wake_by_ref(arc_self: &Arc<Self>) {
                arc_self.0.store(true, Ordering::SeqCst);
            }
        }
        task::waker(Arc::new(Notify(woken_up.clone())))
    };

    let mut context = Context::from_waker(&waker);

    loop {
        // We poll the future continuously until it is either Ready, or the waker stops being
        // invoked during the polling.
        loop {
            if let Poll::Ready(val) = Future::poll(future.as_mut(), &mut context) {
                return val;
            }

            // If the waker has been used during the polling of this future, then we have to pol
            // again.
            if woken_up.swap(false, Ordering::SeqCst) {
                continue;
            } else {
                break;
            }
        }

        let mut state = (&*STATE).lock();
        debug_assert_eq!(state.message_ids.len(), state.wakers.len());

        // `block` indicates whether we should block the thread or just peek. Always `true` during
        // the first iteration, and `false` in further iterations.
        let mut block = true;

        // We process in a loop all pending messages.
        while let Some(msg) = next_notification(&mut state.message_ids, block) {
            block = false;

            match msg {
                DecodedNotification::Response(msg) => {
                    // Value is zero-ed by the kernel.
                    debug_assert_eq!(state.message_ids[msg.index_in_list as usize], 0);
                    if let Some(waker) = state.wakers[msg.index_in_list as usize].take() {
                        waker.wake();
                    }

                    let _was_in = state.pending_messages.insert(msg.message_id, msg);
                    debug_assert!(_was_in.is_none());
                }
                DecodedNotification::Interface(msg) => {
                    // Value is zero-ed by the kernel.
                    debug_assert_eq!(state.message_ids[msg.index_in_list as usize], 0);
                    if let Some(waker) = state.wakers[msg.index_in_list as usize].take() {
                        waker.wake();
                    }

                    let msg = DecodedInterfaceOrDestroyed::Interface(msg);
                    state.interface_messages_queue.push_back(msg);
                }
                DecodedNotification::ProcessDestroyed(msg) => {
                    // Value is zero-ed by the kernel.
                    debug_assert_eq!(state.message_ids[msg.index_in_list as usize], 0);
                    if let Some(waker) = state.wakers[msg.index_in_list as usize].take() {
                        waker.wake();
                    }

                    let msg = DecodedInterfaceOrDestroyed::ProcessDestroyed(msg);
                    state.interface_messages_queue.push_back(msg);
                }
            };
        }

        debug_assert!(!block);
    }
}

lazy_static::lazy_static! {
    // TODO: we're using a Mutex, which is ok for as long as WASM doesn't have threads
    // if WASM ever gets threads and no pre-emptive multitasking, then we might spin forever
    static ref STATE: Mutex<BlockOnState> = {
        Mutex::new(BlockOnState {
            message_ids: Vec::new(),
            wakers: Slab::new(),
            pending_messages: HashMap::with_capacity_and_hasher(6, Default::default()),
            interface_messages_queue: VecDeque::with_capacity(2),
        })
    };
}

/// State of the global `block_on` mechanism.
///
/// This is instantiated only once.
struct BlockOnState {
    /// List of messages for which we are waiting for a response. A pointer to this list is passed
    /// to the kernel.
    message_ids: Vec<u64>,

    /// List whose length is identical to [`BlockOnState::messages_ids`]. For each element in
    /// [`BlockOnState::messages_ids`], contains a corresponding `Waker` that must be waken up
    /// when a response comes.
    wakers: Slab<Option<Waker>>,

    /// Queue of response messages waiting to be delivered.
    ///
    /// > **Note**: We have to maintain this queue as a global variable rather than a per-future
    /// >           channel, otherwise dropping a `Future` would silently drop messages that have
    /// >           already been received.
    pending_messages: HashMap<MessageId, DecodedResponseNotification, BuildNoHashHasher<u64>>,

    /// Queue of interface messages waiting to be delivered.
    ///
    /// > **Note**: We have to maintain this queue as a global variable rather than a per-future
    /// >           channel, otherwise dropping a `Future` would silently drop messages that have
    /// >           already been received.
    interface_messages_queue: VecDeque<DecodedInterfaceOrDestroyed>,
}

/// Checks whether a new message arrives, optionally blocking the thread.
///
/// If `block` is true, then the return value is always `Some`.
///
/// See the [`next_notification`](crate::ffi::next_notification) FFI function for the semantics of
/// `to_poll`.
pub(crate) fn next_notification(to_poll: &mut [u64], block: bool) -> Option<DecodedNotification> {
    next_notification_impl(to_poll, block)
}

#[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
fn next_notification_impl(to_poll: &mut [u64], block: bool) -> Option<DecodedNotification> {
    unsafe {
        let mut out = Vec::<u8>::with_capacity(32);
        loop {
            let ret = crate::ffi::next_notification(
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
            return Some(ffi::decode_notification(&out).unwrap());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn next_notification_impl(_: &mut [u64], _: bool) -> Option<DecodedNotification> {
    unimplemented!()
}
