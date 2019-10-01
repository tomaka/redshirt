use crate::{emit_message, next_message, Message};
use alloc::sync::Arc;
use core::{mem, task::{Context, Poll, Waker}};
use futures::prelude::*;
use parity_scale_codec::{DecodeAll, Encode};

/// Blocks the current thread until the [`Future`](core::future::Future) passed as parameter
/// finishes.
pub fn block_on<T>(future: impl Future<Output = T>) -> T {
    // Implementation note: the function works by emitting a `FutexWait` message, then polling
    // the `Future`, then waiting for answer on that `FutexWait` message. The `Waker` passed when
    // polling emits a `FutexWake` message when `wake` is called.

    pin_utils::pin_mut!(future);

    // Create the waker passed when polling the future.
    let notify = {
        struct Notify { futex: u32 }
        impl futures::task::ArcWake for Notify {
            fn wake_by_ref(arc_self: &Arc<Self>) {
                let futex_wake = threads::ffi::ThreadsMessage::FutexWake(threads::ffi::FutexWake {
                    addr: &arc_self.futex as *const u32 as usize as u32,
                    nwake: 1,
                });
                emit_message(&threads::ffi::INTERFACE, &futex_wake, false).unwrap();
            }
        }

        Arc::new(Notify {
            futex: 0,
        })
    };

    let waker = futures::task::waker(notify.clone());
    let mut context = Context::from_waker(&waker);

    loop {
        // Emit a `FutexWait` message before polling, so that the waker works if it is invoked
        // between `Future::poll` and `next_message`.
        let futex_wait_msg = {
            let msg = threads::ffi::ThreadsMessage::FutexWait(threads::ffi::FutexWait {
                addr: &notify.futex as *const u32 as usize as u32,
                val_cmp: 0,
            });
            emit_message(&threads::ffi::INTERFACE, &msg, true).unwrap().unwrap()
        };

        if let Poll::Ready(val) = Future::poll(future.as_mut(), &mut context) {
            // TODO: cancel `futex_wait_msg`, otherwise we've got a memory leak
            return val;
        }

        // Block the thread until `FutexWake` is emitted, which happens if the `Waker` is woken
        // up.
        // TODO: should we check here whether the result is an appropriate `Response`?
        match next_message(&mut [futex_wait_msg], true) {
            Some(Message::Response(_)) => {},
            Some(Message::Interface(_)) => unreachable!(),
            None => unreachable!(),
        };
    }
}
