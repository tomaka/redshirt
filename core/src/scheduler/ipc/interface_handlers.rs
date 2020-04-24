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

use crate::{EncodedMessage, InterfaceHash, MessageId, Pid, ThreadId};

use alloc::vec::Vec;
use core::mem;
use fnv::FnvBuildHasher;
use hashbrown::{hash_map::Entry, HashMap};
use spinning_top::{Spinlock, SpinlockGuard};

/// Map of how interfaces are handled.
pub struct InterfaceHandlers {
    /// For each interface, which program is fulfilling it.
    interfaces: Spinlock<HashMap<InterfaceHash, InterfaceState, FnvBuildHasher>>,
}

/// Which way an interface is handled.
#[derive(Debug, Clone)]
enum InterfaceState {
    /// Interface has been registered.
    Process(Pid),
    /// Interface hasn't been registered yet, but has been requested.
    Requested(Vec<WaitingForInterface>),
}

/// Something to do after the interface has been registered.
#[derive(Debug, Clone)]
pub enum WaitingForInterface {
    Thread(ThreadId),
    ImmediateDelivery {
        emitter_pid: Pid,
        message_id: Option<MessageId>,
        message: EncodedMessage,
    },
}

pub enum Interface<'a> {
    Registered(Pid),
    Unregistered(UnregisteredInterface<'a>),
}

pub struct UnregisteredInterface<'a> {
    interfaces: SpinlockGuard<'a, HashMap<InterfaceHash, InterfaceState, FnvBuildHasher>>,
    interface: InterfaceHash,
}

impl InterfaceHandlers {
    /// Builds a new empty collection.
    pub fn new() -> Self {
        InterfaceHandlers {
            interfaces: Spinlock::new(Default::default()),
        }
    }

    pub fn get(&self, interface: &InterfaceHash) -> Interface {
        let interfaces = self.interfaces.lock();

        if let Some(InterfaceState::Process(pid)) = interfaces.get(interface) {
            return Interface::Registered(*pid);
        }

        Interface::Unregistered(UnregisteredInterface {
            interfaces,
            interface: interface.clone(),
        })
    }

    /// Sets the handler of the interface.
    pub fn set_interface_handler(
        &self,
        interface: InterfaceHash,
        process: Pid,
    ) -> Result<impl ExactSizeIterator<Item = WaitingForInterface>, ()> {
        let mut interfaces = self.interfaces.lock();
        let mut entry = match interfaces.entry(interface) {
            Entry::Occupied(entry) => entry,
            Entry::Vacant(entry) => {
                entry.insert(InterfaceState::Process(process));
                return Ok(Vec::new().into_iter());
            }
        };

        let requested = match entry.get_mut() {
            InterfaceState::Process(_) => return Err(()),
            InterfaceState::Requested(list) => mem::replace(list, Vec::new()),
        };

        *entry.into_mut() = InterfaceState::Process(process);
        Ok(requested.into_iter())
    }

    /// Sets the given interface as not having a handler. Returns the `Pid` that was registered,
    /// if any.
    pub fn unregister(&self, interface: InterfaceHash) -> Option<Pid> {
        let mut interfaces = self.interfaces.lock();
        match interfaces.entry(interface) {
            Entry::Occupied(e) if matches!(e.get(), InterfaceState::Requested(_)) => {}
            Entry::Vacant(_) => {}
            Entry::Occupied(e) => {
                if let (_, InterfaceState::Process(pid)) = e.remove_entry() {
                    return Some(pid);
                }
            }
        };
        None
    }
}

impl Default for InterfaceHandlers {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> UnregisteredInterface<'a> {
    pub fn insert_waiting_thread(mut self, thread: ThreadId) {
        let entry = self
            .interfaces
            .entry(self.interface.clone())
            .or_insert(InterfaceState::Requested(Vec::new()));

        if let InterfaceState::Requested(list) = entry {
            list.push(WaitingForInterface::Thread(thread));
        } else {
            panic!();
        }
    }

    pub fn insert_waiting_message(
        mut self,
        emitter_pid: Pid,
        message_id: Option<MessageId>,
        message: EncodedMessage,
    ) {
        let entry = self
            .interfaces
            .entry(self.interface.clone())
            .or_insert(InterfaceState::Requested(Vec::new()));

        if let InterfaceState::Requested(list) = entry {
            list.push(WaitingForInterface::ImmediateDelivery {
                emitter_pid,
                message_id,
                message,
            });
        } else {
            panic!();
        }
    }
}
