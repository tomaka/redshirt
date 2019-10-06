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

use crate::{emit_message, InterfaceMessage, Message, ResponseMessage};
use alloc::{collections::VecDeque, sync::Arc};
use core::{
    cell::RefCell,
    mem,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};
use crossbeam::atomic::AtomicCell;
use futures::{prelude::*, task};
use hashbrown::HashMap;
use parity_scale_codec::{DecodeAll, Encode};
use send_wrapper::SendWrapper;

// TODO: document
pub(crate) fn register_message_waker(message_id: u64, waker: Waker) {
    let mut state = (&*STATE).borrow_mut();
    state.message_ids.push(message_id);
    state.wakers.push(waker);
}

// TODO: document
pub(crate) fn peek_interface_message() -> Option<InterfaceMessage> {
    let mut state = (&*STATE).borrow_mut();
    state.interface_messages_queue.pop_front()
}

// TODO: document
pub(crate) fn peek_response(msg_id: u64) -> Option<ResponseMessage> {
    let mut state = (&*STATE).borrow_mut();
    state.pending_messages.remove(&msg_id)
}

/// Blocks the current thread until the [`Future`](core::future::Future) passed as parameter
/// finishes.
pub fn block_on<T>(future: impl Future<Output = T>) -> T {
    // Implementation note: the function works by emitting a `FutexWait` message, then polling
    // the `Future`, then waiting for answer on that `FutexWait` message. The `Waker` passed when
    // polling emits a `FutexWake` message when `wake` is called.

    pin_utils::pin_mut!(future);

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

        let mut state = (&*STATE).borrow_mut();
        debug_assert_eq!(state.message_ids.len(), state.wakers.len());

        // `block` indicates whether we should block the thread or just peek. Always `true` during
        // the first iteration, and `false` in further iterations.
        let mut block = true;

        // We process in a loop all pending messages.
        while let Some(msg) = next_message(&mut state.message_ids, block) {
            block = false;

            match msg {
                Message::Response(msg) => {
                    let _was_in = state.message_ids.remove(msg.index_in_list as usize);
                    debug_assert_eq!(_was_in, 0);

                    let waker = state.wakers.remove(msg.index_in_list as usize);
                    waker.wake();

                    let _was_in = state.pending_messages.insert(msg.message_id, msg);
                    debug_assert!(_was_in.is_none());
                }
                Message::Interface(msg) => {
                    let _was_in = state.message_ids.remove(msg.index_in_list as usize);
                    debug_assert_eq!(_was_in, 0);

                    let waker = state.wakers.remove(msg.index_in_list as usize);
                    waker.wake();

                    state.interface_messages_queue.push_back(msg);
                }
            };
        }

        debug_assert!(!block);
    }
}

lazy_static::lazy_static! {
    static ref STATE: SendWrapper<RefCell<ResponsesState>> = {
        SendWrapper::new(RefCell::new(ResponsesState {
            message_ids: Vec::new(),
            wakers: Vec::new(),
            pending_messages: HashMap::with_capacity(6),
            interface_messages_queue: VecDeque::with_capacity(2),
        }))
    };
}

struct ResponsesState {
    message_ids: Vec<u64>,
    wakers: Vec<Waker>,

    /// Queue of response messages waiting to be delivered.
    ///
    /// > **Note**: We have to maintain this queue as a global variable rather than a per-future
    /// >           channel, otherwise dropping a `Future` would silently drop messages that have
    /// >           already been received.
    pending_messages: HashMap<u64, ResponseMessage>,

    /// Queue of interface messages waiting to be delivered.
    ///
    /// > **Note**: We have to maintain this queue as a global variable rather than a per-future
    /// >           channel, otherwise dropping a `Future` would silently drop messages that have
    /// >           already been received.
    interface_messages_queue: VecDeque<InterfaceMessage>,
}

/// Checks whether a new message arrives, optionally blocking the thread.
///
/// If `block` is true, then the return value is always `Some`.
///
/// See the [`next_message`](crate::ffi::next_message) FFI function for the semantics of
/// `to_poll`.
#[cfg(target_arch = "wasm32")] // TODO: bad
pub(crate) fn next_message(to_poll: &mut [u64], block: bool) -> Option<Message> {
    unsafe {
        let mut out = Vec::with_capacity(32);
        loop {
            let ret = crate::ffi::next_message(
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
            return Some(DecodeAll::decode_all(&out).unwrap());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))] // TODO: bad
pub(crate) fn next_message(to_poll: &mut [u64], block: bool) -> Option<Message> {
    panic!()
}
