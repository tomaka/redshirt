// Copyright (C) 2019-2021  Pierre Krieger
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

//! The [`WaitingThreads`] struct contains a list of threads waiting for notifications to come.
//!
//! Threads can be added to the list using [`WaitingThreads::push`].
//!
//! When a notification is added to the queue (which is outside the scope of this module), one
//! must go through each thread of this list and check whether it can be waken up.
//! The tricky part is that new notifications can continue being pushed while we check the list
//! of threads for wake-ups.
//!
//! In the simple single-threaded situation, call [`WaitingThreads::access`] in order to start
//! iterating over the list. Each thread of the list is returned one by one and can be removed
//! with [`Entry::remove`].
//!
//! In the situation where multiple threads are iterating the list at the same time, the iteration
//! will be shared between the multiple threads. In other words, each thread of the list will be
//! generated only by one of the iterators.
//!
//! If [`WaitingThreads::access`] is called while another thread is already iterating, all the
//! threads that have already been checked will be checked again.

use crate::ThreadId;

use alloc::{collections::VecDeque, vec::Vec};
use core::iter;
use spinning_top::Spinlock;

/// List of threads waiting for notifications.
#[derive(Debug)]
pub struct WaitingThreads {
    inner: Spinlock<WaitingThreadsInner>,
}

#[derive(Debug)]
struct WaitingThreadsInner {
    // TODO: call shrink_to_fit from time to time
    full_list: VecDeque<ThreadId>,
    checks_remaining: VecDeque<ThreadId>,
    current_checks: Vec<ThreadId>,
}

/// An entry in the notifications queue.
#[must_use]
pub struct Entry<'a> {
    waiting_threads: &'a WaitingThreads,
    thread_id: ThreadId,
}

impl WaitingThreads {
    /// Builds a new empty list.
    pub fn new() -> WaitingThreads {
        WaitingThreads {
            inner: Spinlock::new(WaitingThreadsInner {
                full_list: VecDeque::new(),
                checks_remaining: VecDeque::new(),
                current_checks: Vec::new(),
            }),
        }
    }

    /// Adds an element to the list. Will be added to any current iteration if active.
    ///
    /// # Panics
    ///
    /// Panics if the element was already in the list.
    pub fn push(&self, thread_id: ThreadId) {
        let mut inner = self.inner.lock();
        assert!(!inner.full_list.iter().any(|e| *e == thread_id));
        debug_assert!(!inner.current_checks.iter().any(|e| *e == thread_id));
        inner.full_list.push_back(thread_id);
        inner.checks_remaining.push_back(thread_id);
    }

    /// Iterate over the content of the container.
    ///
    /// As explained in the documentation, calling this method guarantees that each entry will be
    /// generated by any of the active iterators.
    ///
    /// # Example situation
    ///
    /// Let's imagine a container with two elements named `A` and `B`.
    ///
    /// You call `access` to start iterating. The iterator returns `A`.
    /// Then you call `access` a second time (while the first iterator is still alive).
    ///
    /// Afterwards, any of the situations below is possible:
    ///
    /// - The first iterator immediately finishes. The second iterator returns `A` and `B`.
    /// - The first iterator returns `A` (again) and `B`. The second iterator immediately finishes.
    /// - The first iterator returns `A` and the second iterator returns `B`. Then they both end.
    /// - The first iterator returns `B` and the second iterator returns `A`. Then the second
    /// iterator ends and the first iterator returns `A` again.
    ///
    /// What is **not** possible is for multiple iterators to return `A` and `B` simultaneously.
    /// In other words, there can never be multiple instances of [`Entry`] alive at the same time
    /// representing the same element.
    pub fn access(&self) -> impl Iterator<Item = Entry> {
        // Reset `checks_remaining` to `full_list`.
        {
            let mut inner = self.inner.lock();
            inner.checks_remaining = inner.full_list.iter().cloned().collect();
        }

        iter::from_fn(move || {
            let mut inner = self.inner.lock();
            let pos = inner
                .checks_remaining
                .iter()
                .position(|t| !inner.current_checks.iter().any(|u| u == t));
            if let Some(pos) = pos {
                let thread_id = inner.checks_remaining.remove(pos).unwrap();
                inner.current_checks.push(thread_id);
                Some(Entry {
                    waiting_threads: self,
                    thread_id,
                })
            } else {
                None
            }
        })
    }
}

impl<'a> Entry<'a> {
    /// Returns the [`ThreadId`] in question.
    pub fn thread_id(&self) -> ThreadId {
        self.thread_id
    }

    /// Removes the entry from the list. This entry will **not** be returned by any other
    /// active iterator, unless it is pushed in the list again.
    pub fn remove(self) {
        let mut inner = self.waiting_threads.inner.lock();
        debug_assert!(!inner.current_checks.iter().any(|e| *e != self.thread_id));
        let pos = inner
            .full_list
            .iter()
            .position(|e| *e == self.thread_id)
            .unwrap();
        inner.full_list.remove(pos);
    }
}

impl<'a> Drop for Entry<'a> {
    fn drop(&mut self) {
        let mut inner = self.waiting_threads.inner.lock();
        let pos = inner
            .current_checks
            .iter()
            .position(|e| *e == self.thread_id)
            .unwrap();
        inner.current_checks.remove(pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id_pool::IdPool;
    use std::{
        collections::HashSet,
        sync::{Arc, Mutex},
    };

    #[test]
    fn fuzz_unique_entry() {
        let queue = Arc::new(WaitingThreads::new());
        let current_accesses = Arc::new(Mutex::new(HashSet::new()));
        let id_pool = Arc::new(IdPool::with_seed([0; 32]));

        let mut threads = Vec::new();

        for _ in 0..16 {
            let queue = queue.clone();
            let current_accesses = current_accesses.clone();
            let id_pool = id_pool.clone();

            threads.push(std::thread::spawn(move || {
                for _ in 0..32 {
                    queue.push(id_pool.assign());
                }

                for thread in queue.access() {
                    {
                        let mut current_accesses = current_accesses.lock().unwrap();
                        assert!(current_accesses.insert(thread.thread_id()));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    {
                        let mut current_accesses = current_accesses.lock().unwrap();
                        assert!(current_accesses.remove(&thread.thread_id()));
                    }
                    drop(thread);
                }
            }));
        }

        for t in threads {
            t.join().unwrap();
        }
    }

    #[test]
    fn access_checks_again_when_active() {
        let queue = WaitingThreads::new();
        let id_pool = IdPool::with_seed([0; 32]);

        let tid1: ThreadId = id_pool.assign();
        queue.push(tid1.clone());

        let mut iter1 = queue.access();

        let elem1 = iter1.next().unwrap();
        assert_eq!(elem1.thread_id(), tid1);

        let mut iter2 = queue.access();
        drop(elem1);

        match (iter1.next(), iter2.next()) {
            (Some(t), None) | (None, Some(t)) => assert_eq!(t.thread_id(), tid1),
            _ => panic!(),
        };
    }

    #[test]
    fn all_are_returned() {
        let queue = Arc::new(WaitingThreads::new());
        let mut remaining_to_access = HashSet::new();

        let id_pool = IdPool::with_seed([0; 32]);
        for _ in 0..32768 {
            let tid: ThreadId = id_pool.assign();
            assert!(remaining_to_access.insert(tid));
            queue.push(tid);
        }

        let remaining_to_access = Arc::new(Mutex::new(remaining_to_access));
        let mut threads = Vec::new();

        for _ in 0..16 {
            let queue = queue.clone();
            let remaining_to_access = remaining_to_access.clone();

            threads.push(std::thread::spawn(move || {
                for thread in queue.access() {
                    remaining_to_access
                        .lock()
                        .unwrap()
                        .remove(&thread.thread_id());
                }
            }));
        }

        for t in threads {
            t.join().unwrap();
        }

        assert!(remaining_to_access.lock().unwrap().is_empty());
    }
}
