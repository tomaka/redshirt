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

//! Futures executor that works on bare metal.

// TODO: only works because we're single-CPU'ed at the moment

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

        wait_for_true(&local_wake.woken_up);
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

// If a different CPU than the local one sets the atomic to true, it will then trigger an
// inter-processor interrupt in order to wake up the local CPU.
//
// What we don't want to happen is:
// - We check the atomic. It is false.
// - A different CPU sets the atomic to true and triggers an interrupt.
// - Interrupt happens locally.
// - We halt, waiting for that interrupt that will never come again.
//
// Because of that, we have to disable interrupts between the moment when we check the atomic's
// value and the moment when we sleep.

#[cfg(target_arch = "x86_64")]
fn wait_for_true(atomic: &atomic::AtomicBool) {
    loop {
        x86_64::instructions::interrupts::disable();
        if atomic.compare_and_swap(true, false, atomic::Ordering::Acquire) {
            x86_64::instructions::interrupts::enable();
            return;
        }

        // An `sti` opcode only takes effect after the *next* opcode, which is `hlt` here.
        // It is not possible for an interrupt to happen between `sti` and `hlt`.
        x86_64::instructions::interrupts::enable();
        x86_64::instructions::hlt();
    }
}

#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
fn wait_for_true(atomic: &atomic::AtomicBool) {
    loop {
        if atomic.compare_and_swap(true, false, atomic::Ordering::Acquire) {
            return;
        }

        // TODO: this is a draft; I don't really know well how ARM interrupts work
        unsafe { asm!("wfe" :::: "volatile") }
    }
}
