use crate::{emit_message, Message, ResponseMessage, InterfaceMessage};
use alloc::sync::Arc;
use core::{
    cell::RefCell,
    mem,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};
use crossbeam::atomic::AtomicCell;
use futures::{prelude::*, task};
use parity_scale_codec::{DecodeAll, Encode};
use send_wrapper::SendWrapper;

// TODO: document
pub(crate) fn register_message_waker(message_id: u64, destination: Arc<AtomicCell<Option<Message>>>, waker: Waker) {
    let mut state = (&*STATE).borrow_mut();
    state.message_ids.push(message_id);
    state.sinks_wakers.push((destination, waker));
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
                continue
            } else {
                break
            }
        }

        let mut state = (&*STATE).borrow_mut();
        // `block` indicates whether we should block the thread or just peek. Always `true` during
        // the first iteration, and `false` in further iterations.
        let mut block = true;

        // We process in a loop all pending messages.
        while let Some(msg) = next_message(&mut state.message_ids, block) {
            println!("got message: {:?}", msg);
            block = false;

            // TODO: index_in_list as a method on Message
            let index_in_list = match msg {
                Message::Response(ResponseMessage { ref index_in_list, .. }) => *index_in_list,
                Message::Interface(InterfaceMessage { ref index_in_list, .. }) => *index_in_list,
            };

            let was_in = state.message_ids.remove(index_in_list as usize);
            assert_eq!(was_in, 0);

            let (mut cell, waker) = state.sinks_wakers.remove(index_in_list as usize);
            cell.store(Some(msg));
            waker.wake();
        }

        debug_assert!(!block);
    }
}

lazy_static::lazy_static! {
    static ref STATE: SendWrapper<RefCell<ResponsesState>> = {
        SendWrapper::new(RefCell::new(ResponsesState {
            message_ids: Vec::new(),
            sinks_wakers: Vec::new(),
        }))
    };
}

struct ResponsesState {
    message_ids: Vec<u64>,
    sinks_wakers: Vec<(Arc<AtomicCell<Option<Message>>>, Waker)>,
}

/// Checks whether a new message arrives, optionally blocking the thread.
///
/// If `block` is true, then the return value is always `Some`.
///
/// See the [`next_message`](crate::ffi::next_message) FFI function for the semantics of
/// `to_poll`.
#[cfg(target_arch = "wasm32")] // TODO: bad
fn next_message(to_poll: &mut [u64], block: bool) -> Option<Message> {
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
fn next_message(to_poll: &mut [u64], block: bool) -> Option<Message> {
    panic!()
}
