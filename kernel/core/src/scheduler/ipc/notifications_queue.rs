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

use crate::scheduler::extrinsics::WaitEntry;
use crate::{EncodedMessage, MessageId};

use core::convert::TryFrom as _;
use hashbrown::HashMap;
use redshirt_syscalls::ffi::NotificationBuilder;
use spinning_top::{Spinlock, SpinlockGuard};

/// Queue of notifications waiting to be delivered.
///
/// One instance of this struct exists for each alive process.
#[derive(Debug)]
pub struct NotificationsQueue {
    // TODO: baka Mutex
    guarded: Spinlock<Guarded>,
}

#[derive(Debug)]
struct Guarded {
    /// The actual list.
    ///
    /// The [`DecodedNotificationRef::index_in_list`](redshirt_syscalls::ffi::DecodedNotificationRef::index_in_list)
    /// field is set to a dummy value, and will be filled before actually delivering the
    /// notification.
    queue: HashMap<MessageId, NotificationBuilder, nohash_hasher::BuildNoHashHasher<u64>>,

    /// Total number of notifications that have been pushed in the notifications queue.
    total_notifications_pushed: u64,
}

/// An entry in the notifications queue.
#[must_use]
pub struct Entry<'a> {
    guarded: SpinlockGuard<'a, Guarded>,
    message_id: MessageId,
    /// Index within the list that was passed as parameter to [`NotificationsQueue::find`].
    index_in_msg_ids: usize,
}

impl NotificationsQueue {
    /// Builds a new empty queue.
    pub fn new() -> NotificationsQueue {
        NotificationsQueue {
            guarded: Spinlock::new(Guarded {
                queue: Default::default(), // TODO: capacity?
                total_notifications_pushed: 0,
            }),
        }
    }

    /// Returns the total number of notifications that have been pushed to this queue.
    pub fn total_notifications_pushed(&self) -> u64 {
        self.guarded.lock().total_notifications_pushed
    }

    /// Pushes a notification at the end of the queue.
    pub fn push(&self, message_id: MessageId, response: Result<EncodedMessage, ()>) {
        let notif = redshirt_syscalls::ffi::build_notification(
            message_id,
            // We use a dummy value here and fill it up later when actually delivering the notif.
            0,
            match &response {
                Ok(r) => Ok(From::from(r)),
                Err(()) => Err(()),
            },
        );

        let mut lock = self.guarded.lock();
        lock.queue.insert(message_id, From::from(notif));
        lock.total_notifications_pushed += 1;
    }

    /// Finds a notification in the list that matches the given indices.
    ///
    /// If an entry is found, its corresponding index within `indices` is stored in the returned
    /// `Entry`.
    // TODO: O(n) complexity!
    pub fn find<'a>(&self, indices: impl IntoIterator<Item = &'a WaitEntry>) -> Option<Entry> {
        let notifications_queue = self.guarded.lock();

        let (index_in_msg_ids, message_id) = {
            indices
                .into_iter()
                .enumerate()
                .filter_map(|(n, e)| match e {
                    WaitEntry::Answer(id) => Some((n, *id)),
                    WaitEntry::Empty => None,
                })
                .find(|(_, id)| notifications_queue.queue.contains_key(id))?
        };

        Some(Entry {
            guarded: notifications_queue,
            message_id,
            index_in_msg_ids,
        })
    }
}

impl<'a> Entry<'a> {
    /// Returns the size in bytes of the notification.
    pub fn size(&self) -> usize {
        self.guarded.queue.get(&self.message_id).unwrap().len()
    }

    // TODO: better method name and doc
    pub fn index_in_msg_ids(&self) -> usize {
        self.index_in_msg_ids
    }

    // TODO: shouldn't be an `EncodedMessage`, that's wrong
    pub fn extract(mut self) -> EncodedMessage {
        let mut notification = self.guarded.queue.remove(&self.message_id).unwrap();

        // Some heuristics in order to reduce memory consumption.
        if self.guarded.queue.capacity() >= 256
            && self.guarded.queue.len() < self.guarded.queue.capacity() / 10
        {
            self.guarded.queue.shrink_to_fit();
        }

        notification.set_index_in_list(u32::try_from(self.index_in_msg_ids).unwrap());
        EncodedMessage(notification.into_bytes())
    }
}
