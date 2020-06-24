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

//! MPMC that works in a standalone environment.

use alloc::sync::Arc;
use core::sync::atomic;
use futures::{prelude::*, task::{AtomicWaker, Context, Poll}};
use hashbrown::HashMap;
use spinning_top::Spinlock;

pub fn channel<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    let shared = Arc::new(Shared {
        next_receiver_id: atomic::AtomicU32::new(1),
        queue: crossbeam_queue::SegQueue::new(),
        wakers: Spinlock::new(HashMap::with_capacity_and_hasher(1, Default::default())),
    });

    let tx = UnboundedSender { shared: shared.clone() };
    let rx = UnboundedReceiver { id: 0, shared: shared.clone() };
    (tx, rx)
}

/// Alternative to `futures::channel::mpsc::UnboundedSender`.
pub struct UnboundedSender<T> {
    shared: Arc<Shared<T>>,
}

/// Alternative to `futures::channel::mpsc::UnboundedReceiver`.
pub struct UnboundedReceiver<T> {
    id: u32,
    shared: Arc<Shared<T>>,
}

struct Shared<T> {
    next_receiver_id: atomic::AtomicU32,
    queue: crossbeam_queue::SegQueue<T>,
    wakers: Spinlock<HashMap<u32, AtomicWaker, fnv::FnvBuildHasher>>,
}

impl<T> UnboundedSender<T> {
    /// Pushes an element on the channel.
    pub fn unbounded_send(&self, item: T) {
        self.shared.queue.push(item);

        let mut wakers = self.shared.wakers.lock();
        for (_, waker) in wakers.iter_mut() {
            waker.wake();
        }
    }
}

impl<T> Clone for UnboundedSender<T> {
    fn clone(&self) -> Self {
        UnboundedSender {
            shared: self.shared.clone(),
        }
    }
}

impl<T> UnboundedReceiver<T> {
    pub async fn next(&self) -> T {
        future::poll_fn(|cx| self.poll_next(cx)).await
    }

    pub fn poll_next(&self, cx: &mut Context) -> Poll<T> {
        if let Ok(item) = self.shared.queue.pop() {
            return Poll::Ready(item);
        }

        {
            let mut wakers = self.shared.wakers.lock();
            wakers.entry(self.id).or_insert(AtomicWaker::new()).register(cx.waker());
        }

        if let Ok(item) = self.shared.queue.pop() {
            return Poll::Ready(item);
        }

        Poll::Pending
    }
}

impl<T> Clone for UnboundedReceiver<T> {
    fn clone(&self) -> Self {
        let id = self.shared.next_receiver_id.fetch_add(1, atomic::Ordering::Relaxed);

        UnboundedReceiver {
            id,
            shared: self.shared.clone(),
        }
    }
}

impl<T> Drop for UnboundedReceiver<T> {
    fn drop(&mut self) {
        let mut wakers = self.shared.wakers.lock();
        wakers.remove(&self.id).unwrap();
    }
}
