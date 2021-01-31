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

// TODO: doc

use alloc::collections::VecDeque;
use core::{convert::TryFrom as _, mem, num::NonZeroU64};
use hashbrown::{hash_map::Entry, HashMap};
use redshirt_syscalls::{InterfaceHash, MessageId, Pid};

pub struct Interfaces {
    // TODO: do something smarter than a spinning lock?
    inner: spinning_top::Spinlock<Inner>,
}

#[derive(Debug)]
struct Inner {
    interfaces: HashMap<InterfaceHash, Interface, fnv::FnvBuildHasher>,
    registrations: slab::Slab<InterfaceRegistration>,
}

#[derive(Debug)]
enum Interface {
    /// Interface has a registered handler.
    ///
    /// Contains an index within [`Inner::registrations`].
    Registered(usize),

    /// Interface has no registered handler yet.
    NotRegistered {
        /// Messages emitted by programs and that haven't been accepted yet are pushed to this
        /// field.
        ///
        /// No limit is enforced on the size of this container. However, since each entry
        /// corresponds to a thread currently being paused, the total number of entries across
        /// all `pending_accept` fields is bounded by the total number of threads across all
        /// processes.
        pending_accept: VecDeque<(MessageId, bool)>,
    },
}

#[derive(Debug)]
struct InterfaceRegistration {
    interface: InterfaceHash,
    pid: Pid,
    /// Messages of type `NextMessage` sent on the interface interface and that must be answered
    /// with the next interface message.
    queries: VecDeque<MessageId>,
    /// If [`InterfaceRegistration::queries`] is empty, messages emitted by programs and that
    /// haven't been accepted yet are pushed to this field.
    pending_accept: VecDeque<(MessageId, bool)>,
}

impl Interfaces {
    pub fn new() -> Self {
        Interfaces {
            inner: spinning_top::Spinlock::new(Inner {
                interfaces: Default::default(),
                registrations: {
                    // Registration IDs are of the type `NonZeroU64`.
                    // The list of registrations starts with an entry at index `0` in order for
                    // generated registration IDs to never be equal to 0.
                    let mut registrations = slab::Slab::default();
                    let _id = registrations.insert(InterfaceRegistration {
                        interface: InterfaceHash::from_raw_hash(Default::default()),
                        pid: 0xdeadbeef.into(), // TODO: ?!
                        queries: VecDeque::new(),
                        pending_accept: VecDeque::new(),
                    });
                    assert_eq!(_id, 0);
                    registrations
                },
            }),
        }
    }

    /// Called when a process requests to deliver a message to an interface handler.
    pub fn emit_interface_message(
        &self,
        interface_hash: &InterfaceHash,
        message_id: MessageId,
        emitter_pid: Pid,
        needs_answer: bool,
        immediate: bool,
    ) -> EmitInterfaceMessage {
        let mut interfaces = self.inner.lock();
        let interfaces = &mut *interfaces; // Avoids borrow errors.

        let entry = match interfaces.interfaces.entry(interface_hash.clone()) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(_) if immediate => {
                return EmitInterfaceMessage::Reject;
            }
            Entry::Vacant(e) => {
                e.insert(Interface::NotRegistered {
                    pending_accept: VecDeque::with_capacity(16), // TODO: capacity
                })
            }
        };

        match entry {
            Interface::Registered(registration_id) => {
                let registration = &mut interfaces.registrations[*registration_id];
                if let Some(query_message_id) = registration.queries.pop_front() {
                    debug_assert!(registration.pending_accept.is_empty());
                    EmitInterfaceMessage::Deliver(MessageDelivery {
                        to_deliver_message_id: message_id,
                        interface: registration.interface.clone(),
                        needs_answer,
                        query_message_id,
                        recipient_pid: registration.pid,
                    })
                } else if immediate {
                    EmitInterfaceMessage::Reject
                } else {
                    registration
                        .pending_accept
                        .push_back((message_id, needs_answer));
                    EmitInterfaceMessage::Queued
                }
            }
            Interface::NotRegistered { pending_accept } => {
                if immediate {
                    EmitInterfaceMessage::Reject
                } else {
                    // TODO: is this unbounded queue attackable?
                    pending_accept.push_back((message_id, needs_answer));
                    EmitInterfaceMessage::Queued
                }
            }
        }
    }

    /// Called when an interface handler emits a request for the next message that arrives on an
    /// interface.
    ///
    /// Must be passed a [`RegistrationId`] and the [`Pid`] that the registration is expected to
    /// belong to. The method verifies that the ownership matches.
    ///
    /// On success, can return a [`MessageDelivery`] representing a delivery of a certain
    /// message earlier pushed using [`Interfaces::emit_interface_message`] to
    /// `expected_registrer_pid` by answering `query_message_id`.
    pub fn emit_message_query(
        &self,
        registration_id: RegistrationId,
        query_message_id: MessageId,
        expected_registerer_pid: Pid,
    ) -> Result<Option<MessageDelivery>, ()> {
        let registration_id = match usize::try_from(registration_id.0.get()) {
            Ok(v) => v,
            Err(_) => return Err(()),
        };

        let mut inner = self.inner.lock();

        if let Some(registration) = inner.registrations.get_mut(registration_id) {
            if registration.pid == expected_registerer_pid {
                if let Some((msg, needs_answer)) = registration.pending_accept.pop_front() {
                    debug_assert!(registration.queries.is_empty());
                    Ok(Some(MessageDelivery {
                        to_deliver_message_id: msg,
                        interface: registration.interface.clone(),
                        needs_answer,
                        query_message_id,
                        recipient_pid: registration.pid,
                    }))
                } else {
                    registration.queries.push_back(query_message_id);
                    Ok(None)
                }
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    }

    /// Sets the handler of the given interface hash.
    ///
    /// On success, returns a [`RegistrationId`] to pass later to refer to that registration.
    ///
    /// Returns an error if there already exists a handler for this interface.
    pub fn set_interface_handler(
        &self,
        interface_hash: InterfaceHash,
        pid: Pid,
    ) -> Result<NonZeroU64, redshirt_interface_interface::ffi::InterfaceRegisterError> {
        let mut interfaces = self.inner.lock();
        let interfaces = &mut *interfaces;

        match interfaces.interfaces.entry(interface_hash) {
            Entry::Occupied(mut entry) => {
                let interface = entry.key().clone();
                match entry.get_mut() {
                    Interface::Registered(_) =>
                        Err(redshirt_interface_interface::ffi::InterfaceRegisterError::AlreadyRegistered),
                    Interface::NotRegistered { pending_accept } => {
                        let id = interfaces.registrations.insert(InterfaceRegistration {
                            pid,
                            interface,
                            queries: VecDeque::with_capacity(16),  // TODO: be less magic with capacity
                            pending_accept: mem::take(pending_accept),
                        });
                        entry.insert(Interface::Registered(id));
                        Ok(NonZeroU64::new(u64::try_from(id).unwrap()).unwrap())
                    }
                }
            }
            Entry::Vacant(entry) => {
                let id = interfaces.registrations.insert(InterfaceRegistration {
                    pid,
                    interface: entry.key().clone(),
                    queries: VecDeque::with_capacity(16), // TODO: be less magic with capacity
                    pending_accept: VecDeque::with_capacity(16), // TODO: be less magic with capacity
                });
                entry.insert(Interface::Registered(id));
                Ok(NonZeroU64::new(u64::try_from(id).unwrap()).unwrap())
            }
        }
    }
}

impl Default for Interfaces {
    fn default() -> Self {
        Interfaces::new()
    }
}

/// Delivery of a message to a handler.
pub struct MessageDelivery {
    /// Identifier of the message to be delivered.
    pub to_deliver_message_id: MessageId,
    /// Registered interface the message concerns.
    // TODO: is this needed? programs should be able to deduce this from the message id
    pub interface: InterfaceHash,
    /// True if the message in `to_deliver_message_id` expects an answer.
    pub needs_answer: bool,
    pub query_message_id: MessageId,
    pub recipient_pid: Pid,
}

/// Outcome of [`Interfaces::emit_interface_message`].
#[must_use]
pub enum EmitInterfaceMessage {
    /// Message pushed on the interface can be instantly accepted and delivered.
    Deliver(MessageDelivery),
    /// Message should be immediately rejected. Can only happen if `immediate` is `true`.
    Reject,
    /// Message has been queued and might later be delivered when
    /// [`Interfaces::emit_message_query`] is called. Can only happen if `immediate` is `false`.
    Queued,
}

/// Identifier of an interface registration.
///
/// See [`Interfaces::set_interface_handler`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegistrationId(NonZeroU64);

impl From<NonZeroU64> for RegistrationId {
    fn from(v: NonZeroU64) -> RegistrationId {
        RegistrationId(v)
    }
}

impl From<RegistrationId> for NonZeroU64 {
    fn from(v: RegistrationId) -> NonZeroU64 {
        v.0
    }
}
