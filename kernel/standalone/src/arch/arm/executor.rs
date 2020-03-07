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

//! Futures executor that works on bare metal.

use alloc::sync::Arc;
use core::future::Future;
use core::sync::atomic;
use core::task::{Context, Poll};
use futures::task::{waker, ArcWake};

/// Waits for the `Future` to resolve to a value.
///
/// This function is similar to [`futures::executor::block_on`].
pub fn block_on<R>(future: impl Future<Output = R>) -> R {
    futures::pin_mut!(future);

    let local_wake = Arc::new(LocalWake {
        woken_up: atomic::AtomicBool::new(false),
    });

    let waker = waker(local_wake.clone());
    let mut context = Context::from_waker(&waker);

    loop {
        if let Poll::Ready(val) = Future::poll(future.as_mut(), &mut context) {
            return val;
        }

        // Loop until `woken_up` is true.
        loop {
            if local_wake
                .woken_up
                .compare_and_swap(true, false, atomic::Ordering::Acquire)
            {
                break;
            }

            // Enter a low-power state and wait for an event to happen.
            //
            // ARM CPUs have a non-accessible 1bit "event register" that is set when an event
            // happens and cleared only by the `wfe` instruction.
            //
            // Thanks to this, if an event happens between the moment when we check the value of
            // `local_waken.woken_up` and the moment when we call `wfe`, then the `wfe`
            // instruction will immediately return and we will check the value again.
            unsafe { asm!("wfe" :::: "volatile") }
        }
    }
}

struct LocalWake {
    woken_up: atomic::AtomicBool,
}

impl ArcWake for LocalWake {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        unsafe {
            arc_self.woken_up.store(true, atomic::Ordering::Release);
            // Wakes up all the CPUs that called `wfe`.
            // Note that this wakes up *all* CPUs, but the ARM architecture doesn't provide any
            // way to target a single CPU for wake-up.
            asm!("dsb sy ; sev" :::: "volatile")
        }
    }
}
