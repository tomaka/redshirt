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

// TODO: only works because we're single-CPU'ed at the moment

use crate::arch::x86_64::apic::ApicControl;

use alloc::sync::Arc;
use core::future::Future;
use core::sync::atomic;
use core::task::{Context, Poll};
use futures::task::{waker, ArcWake};

/// Waits for the `Future` to resolve to a value.
///
/// This function is similar to [`futures::executor::block_on`].
pub fn block_on<R>(apic: &Arc<ApicControl>, future: impl Future<Output = R>) -> R {
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

        loop {
            x86_64::instructions::interrupts::disable();
            if local_wake
                .woken_up
                .compare_and_swap(true, false, atomic::Ordering::Acquire)
            {
                x86_64::instructions::interrupts::enable();
                break;
            }

            // An `sti` opcode only takes effect after the *next* opcode, which is `hlt` here.
            // It is not possible for an interrupt to happen between `sti` and `hlt`.
            x86_64::instructions::interrupts::enable();
            x86_64::instructions::hlt();
        }
    }
}

struct LocalWake {
    woken_up: atomic::AtomicBool,
}

impl ArcWake for LocalWake {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.woken_up.store(true, atomic::Ordering::Release);
        // TODO: wake up original CPU, once we're multi-CPU'ed
    }
}
