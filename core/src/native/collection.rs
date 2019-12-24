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

use crate::native::traits::{NativeProgramEvent, NativeProgramMessageIdWrite, NativeProgramRef};

use alloc::{boxed::Box, vec::Vec};
use core::{mem, pin::Pin, task::Context, task::Poll};
use futures::prelude::*;
use hashbrown::{hash_map::Entry, HashMap};
use redshirt_syscalls_interface::{MessageId, Pid};

/// Collection of objects that implement the [`NativeProgram`] trait.
pub struct NativeProgramsCollection {
    // TODO: add ` + 'a` in the `Box`, to allow non-'static programs
    processes: HashMap<Pid, Box<dyn AdapterAbstract + Send>>,
}

/// Wraps around a [`NativeProgram`].
struct Adapter<T> {
    inner: T,
    //registered_interfaces: HashSet<>,
}

/// Abstracts over [`Adapter`] so that we can box it.
trait AdapterAbstract {
    fn poll_next_event<'a>(
        &'a self,
        cx: &mut Context,
    ) -> Poll<NativeProgramEvent<Box<dyn AbstractMessageIdWrite + 'a>>>;
    fn deliver_interface_message(
        &self,
        interface: [u8; 32],
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: Vec<u8>,
    ) -> Result<(), Vec<u8>>;
    fn process_destroyed(&self, pid: Pid);
}

impl<T> AdapterAbstract for Adapter<T>
where
    for<'r> &'r T: NativeProgramRef<'r>,
{
    fn poll_next_event<'a>(
        &'a self,
        cx: &mut Context,
    ) -> Poll<NativeProgramEvent<Box<dyn AbstractMessageIdWrite + 'a>>> {
        let future = (&self.inner).next_event();
        futures::pin_mut!(future);
        match future.poll(cx) {
            Poll::Ready(NativeProgramEvent::Emit {
                interface,
                message_id_write,
                message,
            }) => Poll::Ready(NativeProgramEvent::Emit {
                interface,
                message,
                message_id_write: message_id_write.map(|w| Box::new(Some(w)) as Box<_>),
            }),
            Poll::Ready(NativeProgramEvent::CancelMessage { message_id }) => {
                Poll::Ready(NativeProgramEvent::CancelMessage { message_id })
            }
            Poll::Ready(NativeProgramEvent::Answer { message_id, answer }) => {
                Poll::Ready(NativeProgramEvent::Answer { message_id, answer })
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn deliver_interface_message(
        &self,
        interface: [u8; 32],
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: Vec<u8>,
    ) -> Result<(), Vec<u8>> {
        // FIXME: don't assume `interface` is handled
        self.inner
            .interface_message(interface, message_id, emitter_pid, message);
        Ok(())
    }

    fn process_destroyed(&self, pid: Pid) {
        self.inner.process_destroyed(pid);
    }
}

trait AbstractMessageIdWrite {
    fn acknowledge(&mut self, id: MessageId);
}

impl<T: NativeProgramMessageIdWrite> AbstractMessageIdWrite for Option<T> {
    fn acknowledge(&mut self, id: MessageId) {
        self.take().unwrap().acknowledge(id);
    }
}

/// Event generated by a [`NativeProgram`].
pub enum NativeProgramsCollectionEvent<'a> {
    /// Request to emit a message.
    Emit {
        interface: [u8; 32],
        pid: Pid,
        message: Vec<u8>,
        message_id_write: Option<NativeProgramsCollectionMessageIdWrite<'a>>,
    },
    /// Request to cancel a previously-emitted message.
    CancelMessage { message_id: MessageId },
    Answer {
        message_id: MessageId,
        answer: Result<Vec<u8>, ()>,
    },
}

pub struct NativeProgramsCollectionMessageIdWrite<'collec> {
    write: Box<dyn AbstractMessageIdWrite + 'collec>,
}

impl NativeProgramsCollection {
    pub fn new() -> Self {
        NativeProgramsCollection {
            processes: HashMap::new(),
        }
    }

    /// Adds a program to the collection.
    ///
    /// # Panic
    ///
    /// Panics if the `pid` already exists in this collection.
    ///
    // TODO: I don't think the lifetimes are correct
    pub fn push<T>(&mut self, pid: Pid, program: T)
    where
        T: Send + 'static,
        for<'r> &'r T: NativeProgramRef<'r>,
    {
        let adapter = Box::new(Adapter { inner: program });

        match self.processes.entry(pid) {
            Entry::Occupied(_) => panic!(),
            Entry::Vacant(e) => e.insert(adapter),
        };

        // We assume that `push` is only ever called at initialization.
        self.processes.shrink_to_fit();
    }

    pub fn next_event<'collec>(
        &'collec self,
    ) -> impl Future<Output = NativeProgramsCollectionEvent<'collec>> + 'collec {
        future::poll_fn(move |cx| {
            for (pid, process) in self.processes.iter() {
                match process.poll_next_event(cx) {
                    Poll::Pending => {}
                    Poll::Ready(NativeProgramEvent::Emit {
                        interface,
                        message_id_write,
                        message,
                    }) => {
                        return Poll::Ready(NativeProgramsCollectionEvent::Emit {
                            pid: *pid,
                            interface,
                            message,
                            message_id_write: message_id_write
                                .map(|w| NativeProgramsCollectionMessageIdWrite { write: w }),
                        })
                    }
                    Poll::Ready(NativeProgramEvent::CancelMessage { message_id }) => {
                        return Poll::Ready(NativeProgramsCollectionEvent::CancelMessage {
                            message_id,
                        })
                    }
                    Poll::Ready(NativeProgramEvent::Answer { message_id, answer }) => {
                        return Poll::Ready(NativeProgramsCollectionEvent::Answer {
                            message_id,
                            answer,
                        })
                    }
                }
            }

            Poll::Pending
        })
    }

    /// Notify the [`NativeProgram`] that a message has arrived on one of the interface that it
    /// has registered.
    pub fn interface_message(
        &self,
        interface: [u8; 32],
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        mut message: Vec<u8>,
    ) {
        for process in self.processes.values() {
            let mut msg = mem::replace(&mut message, Vec::new());
            match process.deliver_interface_message(interface, message_id, emitter_pid, msg) {
                Ok(_) => return,
                Err(msg) => message = msg,
            }
        }

        panic!() // TODO: what to do here?
    }

    /// Notify the [`NativeProgram`]s that the program with the given [`Pid`] has terminated.
    pub fn process_destroyed(&mut self, pid: Pid) {
        for process in self.processes.values() {
            process.process_destroyed(pid);
        }
    }

    /// Notify the appropriate [`NativeProgram`] of a response to a message that it has previously
    /// emitted.
    pub fn message_response(&self, message_id: MessageId, response: Vec<u8>) {
        unimplemented!()
    }
}

impl<'a> NativeProgramMessageIdWrite for NativeProgramsCollectionMessageIdWrite<'a> {
    fn acknowledge(mut self, message_id: MessageId) {
        self.write.acknowledge(message_id);
    }
}

// TODO: impl<'a> NativeProgram<'a> for NativeProgramsCollection<'a>
