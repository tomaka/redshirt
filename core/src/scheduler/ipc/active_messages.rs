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

use crate::{id_pool::IdPool, MessageId, Pid};

use hashbrown::{hash_map::Entry, HashMap};
use nohash_hasher::BuildNoHashHasher;
use spinning_top::Spinlock;

/// Collection of active messages waiting for an answer.
pub struct ActiveMessages {
    /// Pool of identifiers where `MessageId`s are allocated.
    id_pool: IdPool,

    /// Messages that are waiting for a response, and Pid of the emitter of the message.
    // TODO: doc about hash safety
    // TODO: mutex not great /!\
    active_messages: Spinlock<HashMap<MessageId, Pid, BuildNoHashHasher<u64>>>,
}

impl ActiveMessages {
    /// Builds a new empty collection.
    pub fn new() -> Self {
        ActiveMessages {
            id_pool: IdPool::new(),
            active_messages: Spinlock::new(HashMap::default()),
        }
    }

    /// Creates a new message, emitted by the given [`Pid`].
    pub fn add_message(&self, emitter: Pid) -> MessageId {
        loop {
            let id = self.id_pool.assign();
            let mut active_messages = self.active_messages.lock();
            match active_messages.entry(id) {
                Entry::Occupied(_) => continue,
                Entry::Vacant(e) => e.insert(emitter),
            };
            break id;
        }
    }

    /// Removes the given message from the list. Returns the `Pid` that has emitted it, or `None`
    /// if the message was not in the list.
    pub fn remove(&self, message_id: MessageId) -> Option<Pid> {
        self.active_messages.lock().remove(&message_id)
    }

    /// Removes the given message, but only if it has been emitted by the given `Pid`.
    pub fn remove_if_emitted_by(&self, message_id: MessageId, pid: Pid) {
        let mut active_messages = self.active_messages.lock();
        if let Entry::Occupied(entry) = active_messages.entry(message_id) {
            if *entry.get() == pid {
                entry.remove();
            }
        }
    }
}

impl Default for ActiveMessages {
    fn default() -> Self {
        Self::new()
    }
}
