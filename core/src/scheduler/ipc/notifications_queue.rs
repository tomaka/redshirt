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

use crate::scheduler::extrinsics::WaitEntry;
use crate::{EncodedMessage, InterfaceHash, MessageId, Pid};

use alloc::collections::VecDeque;
use core::{convert::TryFrom, num::NonZeroU64};
use redshirt_syscalls::ffi::NotificationBuilder;
use spinning_top::{Spinlock, SpinlockGuard};

/// Queue of notifications waiting to be delivered.
#[derive(Debug)]
pub struct NotificationsQueue {
    /// The actual list.
    ///
    /// The [`DecodedResponseNotification::index_in_list`](redshirt_syscalls::ffi::DecodedResponseNotification::index_in_list)
    /// and [`DecodedInterfaceNotification::index_in_list`](redshirt_syscalls::ffi::DecodedInterfaceNotification::index_in_list)
    /// fields are set to a dummy value, and will be filled before actually delivering the
    /// notification.
    // TODO: call shrink_to_fit from time to time
    // TODO: baka Mutex
    notifications_queue: Spinlock<VecDeque<NotificationBuilder>>,
}

/// An entry in the notifications queue.
#[must_use]
pub struct Entry<'a> {
    queue: SpinlockGuard<'a, VecDeque<NotificationBuilder>>,
    index_in_queue: usize,
    /// Index within the list that was passed as parameter to [`NotificationsQueue::find`].
    index_in_msg_ids: usize,
}

impl NotificationsQueue {
    /// Builds a new empty queue.
    pub fn new() -> NotificationsQueue {
        NotificationsQueue {
            notifications_queue: Spinlock::new(VecDeque::new()),
        }
    }

    /// Destroys `self` and returns the list of all interface notifications in the queue
    /// containing a `MessageId`.
    pub fn into_pending_interface_notifications_messages(self) -> impl Iterator<Item = MessageId> {
        let notifications_queue = self.notifications_queue.into_inner();
        notifications_queue.into_iter().filter_map(|notif| {
            if let NotificationBuilder::Interface(notif) = notif {
                notif.message_id()
            } else {
                None
            }
        })
    }

    /// Adds an interface notification at the end of the queue.
    pub fn push_interface_notification(
        &self,
        interface: &InterfaceHash,
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        let notif = redshirt_syscalls::ffi::build_interface_notification(
            &interface,
            message_id,
            emitter_pid,
            // We use a dummy value here and fill it up later when actually delivering the notif.
            0,
            &message,
        );

        self.notifications_queue.lock().push_back(From::from(notif));
    }

    /// Pushes a response notification at the end of the queue.
    pub fn push_response(&self, message_id: MessageId, response: Result<EncodedMessage, ()>) {
        let notif = redshirt_syscalls::ffi::build_response_notification(
            message_id,
            // We use a dummy value here and fill it up later when actually delivering the notif.
            0,
            match &response {
                Ok(r) => Ok(r),
                Err(()) => Err(()),
            },
        );

        self.notifications_queue.lock().push_back(From::from(notif));
    }

    /// Pushes a notification about a process being destroyed at the end of the queue.
    pub fn push_process_destroyed_notification(&self, pid: Pid) {
        let notif = redshirt_syscalls::ffi::build_process_destroyed_notification(
            pid,
            // We use a dummy value here and fill it up later when actually delivering the notif.
            0,
        );

        self.notifications_queue.lock().push_back(From::from(notif));
    }

    /// Finds a notification in the list that matches the given indices.
    ///
    /// If an entry is found, its corresponding index within `indices` is stored in the returned
    /// `Entry`.
    // TODO: something better than a slice as parameter?
    pub fn find(&self, indices: &[WaitEntry]) -> Option<Entry> {
        let notifications_queue = self.notifications_queue.lock();

        let mut index_in_queue = 0;
        let index_in_msg_ids = loop {
            if index_in_queue >= notifications_queue.len() {
                // No notification found.
                return None;
            }

            // For that notification in queue, build the value that must be in `msg_ids` in order
            // to match.
            let wait_entry = match &notifications_queue[index_in_queue] {
                NotificationBuilder::Interface(_) | NotificationBuilder::ProcessDestroyed(_) => {
                    WaitEntry::InterfaceOrProcDestroyed
                }
                NotificationBuilder::Response(response) => {
                    debug_assert!(u64::from(response.message_id()) >= 2);
                    WaitEntry::Answer(response.message_id())
                }
            };

            if let Some(p) = indices.iter().position(|id| *id == wait_entry) {
                break p;
            }

            index_in_queue += 1;
        };

        Some(Entry {
            queue: notifications_queue,
            index_in_queue,
            index_in_msg_ids,
        })
    }
}

impl<'a> Entry<'a> {
    /// Returns the size in bytes of the notification.
    pub fn size(&self) -> usize {
        self.queue[self.index_in_queue].len()
    }

    // TODO: better method name and doc
    pub fn index_in_msg_ids(&self) -> usize {
        self.index_in_msg_ids
    }

    // TODO: shouldn't be an `EncodedMessage`, that's wrong
    pub fn extract(mut self) -> EncodedMessage {
        let mut notification = self.queue.remove(self.index_in_queue).unwrap();
        notification.set_index_in_list(u32::try_from(self.index_in_msg_ids).unwrap());
        EncodedMessage(notification.into_bytes())
    }
}
