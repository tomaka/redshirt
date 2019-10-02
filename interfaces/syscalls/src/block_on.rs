use crate::{emit_message, next_message, Message, ResponseMessage, InterfaceMessage};
use alloc::sync::Arc;
use core::{
    cell::RefCell,
    mem,
    task::{Context, Poll, Waker},
};
use crossbeam::atomic::AtomicCell;
use futures::prelude::*;
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

    let mut context = Context::from_waker(futures::task::noop_waker_ref());

    loop {
        if let Poll::Ready(val) = Future::poll(future.as_mut(), &mut context) {
            return val;
        }

        let mut state = (&*STATE).borrow_mut();
        let msg = next_message(&mut state.message_ids, true).unwrap();
        println!("got message: {:?}", msg);

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
