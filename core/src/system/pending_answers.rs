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

//! Delivered messages waiting to be answered.
//!
//! The [`PendingAnswers`] struct holds a list of messages that have been successfully delivered
//! to interface handles but haven't been answered yet.

use alloc::vec::Vec;
use hashbrown::HashMap;
use nohash_hasher::BuildNoHashHasher;
use redshirt_syscalls::{MessageId, Pid};

pub struct PendingAnswers {
    // TODO: smarter than a spinloop?
    inner: spinning_top::Spinlock<Inner>,
}

struct Inner {
    // TODO: call shrink_to_fit from time to time?
    messages: HashMap<MessageId, Pid, BuildNoHashHasher<u64>>,
}

impl PendingAnswers {
    pub fn new() -> Self {
        PendingAnswers {
            inner: spinning_top::Spinlock::new(Inner {
                messages: Default::default(),
            }),
        }
    }

    pub fn add(&self, message_id: MessageId, answerer_pid: Pid) {
        let _inserted = self.inner.lock().messages.insert(message_id, answerer_pid);
        debug_assert!(_inserted.is_none());
    }

    pub fn remove(&self, message_id: &MessageId, if_answerer_equal: &Pid) -> Result<(), ()> {
        let mut inner = self.inner.lock();
        match inner.messages.remove(message_id) {
            Some(pid) if pid == *if_answerer_equal => Ok(()),
            Some(pid) => {
                // Cancel the removal.
                inner.messages.insert(message_id.clone(), pid);
                Err(())
            }
            None => Err(()),
        }
    }

    /// Removes from the collection all messages whose answerer is the given PID.
    pub fn drain_by_answerer(&self, answerer_pid: &Pid) -> Vec<MessageId> {
        // TODO: O(n) complexity
        let mut inner = self.inner.lock();

        let list = inner
            .messages
            .iter()
            .filter(|(_, a)| *a == answerer_pid)
            .map(|(m, _)| *m)
            .collect::<Vec<_>>();

        for message in &list {
            let _was_removed = inner.messages.remove(message);
            debug_assert!(_was_removed.is_some());
        }

        list
    }
}

impl Default for PendingAnswers {
    fn default() -> Self {
        PendingAnswers::new()
    }
}
