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

use crate::id_pool::IdPool;
use crate::module::Module;
use crate::scheduler::{processes, vm, Pid, ThreadId};
use crate::sig;
use crate::signature::Signature;

use alloc::{borrow::Cow, collections::VecDeque, vec, vec::Vec};
use byteorder::{ByteOrder as _, LittleEndian};
use core::{convert::TryFrom, marker::PhantomData};
use hashbrown::{hash_map::Entry, HashMap};
use parity_scale_codec::Encode;
use smallvec::SmallVec;

/// Handles scheduling processes and inter-process communications.
pub struct Core<T> {
    /// List of running processes.
    processes: processes::ProcessesCollection<Extrinsic<T>, Process, Thread>,

    /// For each non-registered interface, which threads are waiting for it.
    interface_waits: HashMap<[u8; 32], SmallVec<[ThreadId; 4]>>,

    /// For each interface, which program is fulfilling it.
    interfaces: HashMap<[u8; 32], InterfaceHandler>,

    /// Pool of identifiers for messages.
    message_id_pool: IdPool,

    /// List of messages that have been emitted either by a process or by the external API and
    /// that are waiting for a response.
    // TODO: doc about hash safety
    // TODO: call shrink_to from time to time
    messages_to_answer: HashMap<u64, MessageEmitter>,
}

/// Which way an interface is handled.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InterfaceHandler {
    /// Interface has been registered using [`Core::set_interface_handler`].
    Process(Pid),
    /// Interface has been registered using [`CoreBuilder::with_interface_handler`].
    External,
}

/// What was the source fo a message.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MessageEmitter {
    Process(Pid),
    External,
}

/// Possible function available to processes.
/// The [`External`](Extrinsic::External) variant corresponds to functions that the user of the
/// [`Core`] registers. The rest are handled by the [`Core`] itself.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Extrinsic<T> {
    NextMessage,
    EmitMessage,
    EmitAnswer,
    CancelMessage,
    External(T),
}

/// Prototype for a `Core` under construction.
pub struct CoreBuilder<T> {
    /// See the corresponding field in `Core`.
    interfaces: HashMap<[u8; 32], InterfaceHandler>,
    /// Builder for the [`processes`][Core::processes] field in `Core`.
    inner_builder: processes::ProcessesCollectionBuilder<Extrinsic<T>>,
}

/// Outcome of calling [`run`](Core::run).
// TODO: #[derive(Debug)]
pub enum CoreRunOutcome<'a, T> {
    /// A program has stopped, either because the main function has stopped or a problem has
    /// occurred.
    ProgramFinished {
        /// Id of the program that has stopped.
        process: Pid,

        /// List of messages emitted using [`Core::emit_interface_message_answer`] that were
        /// supposed to be handled by the process that has just terminated.
        unhandled_messages: Vec<u64>,

        /// List of messages for which a [`CoreRunOutcome::InterfaceMessage`] has been emitted
        /// but that no loner need answering.
        cancelled_messages: Vec<u64>,

        /// List of interfaces that were registered by th process and no longer are.
        unregistered_interfaces: Vec<[u8; 32]>,

        /// How the program ended. If `Ok`, it has gracefully terminated. If `Err`, something
        /// bad happened.
        // TODO: force Ok to i32?
        outcome: Result<Option<wasmi::RuntimeValue>, wasmi::Trap>,
    },

    /// A thread has called a function registered using [`CoreBuilder::with_extrinsic`].
    ///
    /// The thread is now sleeping and must be waken up using
    /// [`CoreThread::resolve_extrinsic_call`].
    ThreadWaitExtrinsic {
        /// Thread that has made the call.
        thread: CoreThread<'a, T>,

        /// Identifier for the extrinsic that was passed to [`CoreBuilder::with_extrinsic`].
        extrinsic: T,

        /// Parameters passed to the extrinsic call. Guaranteed to match the signature that was
        /// passed to [`CoreBuilder::with_extrinsic`].
        params: Vec<wasmi::RuntimeValue>,
    },

    /// Thread has tried to emit a message on an interface that isn't registered. The thread is
    /// now in sleep mode. You can either wake it up by calling [`set_interface_handler`], or
    /// resume the thread with an "interface not available error" by calling . // TODO
    ThreadWaitUnavailableInterface {
        /// Thread that emitted the message.
        thread: CoreThread<'a, T>,

        /// Interface that the thread is trying to access.
        interface: [u8; 32],
    },

    /// A process has emitted a message on an interface registered using
    /// [`CoreBuilder::with_interface_handler`].
    InterfaceMessage {
        pid: Pid,
        message_id: Option<u64>,
        interface: [u8; 32],
        message: Vec<u8>,
    },

    /// Response to a message emitted using [`Core::emit_interface_message_answer`].
    MessageResponse {
        message_id: u64,
        pid: Pid,
        response: Vec<u8>,
    },

    /// Nothing to do. No thread is ready to run.
    Idle,
}

/// Because of lifetime issues, this is the same as `CoreRunOutcome` but that holds `Pid`s instead
/// of `CoreProcess`es.
// TODO: remove this enum and solve borrowing issues
enum CoreRunOutcomeInner<T> {
    ProgramFinished {
        process: Pid,
        unhandled_messages: Vec<u64>,
        cancelled_messages: Vec<u64>,
        unregistered_interfaces: Vec<[u8; 32]>,
        outcome: Result<Option<wasmi::RuntimeValue>, wasmi::Trap>,
    },
    ThreadWaitExtrinsic {
        thread: ThreadId,
        extrinsic: T,
        params: Vec<wasmi::RuntimeValue>,
    },
    ThreadWaitUnavailableInterface {
        thread: ThreadId,
        interface: [u8; 32],
    },
    InterfaceMessage {
        // TODO: `pid` is redundant with `message_id`; should just be a better API with an `Event` handle struct
        pid: Pid,
        message_id: Option<u64>,
        interface: [u8; 32],
        message: Vec<u8>,
    },
    MessageResponse {
        pid: Pid,
        message_id: u64,
        response: Vec<u8>,
    },
    LoopAgain,
    Idle,
}

/// Additional information about a process.
#[derive(Debug)]
struct Process {
    /// Messages available for retrieval by the process by calling `next_message`.
    ///
    /// Note that the [`ResponseMessage::index_in_list`](nametbd_syscalls_interface::ffi::ResponseMessage::index_in_list)
    /// and [`InterfaceMessage::index_in_list`](nametbd_syscalls_interface::ffi::InterfaceMessage::index_in_list) fields are
    /// set to a dummy value, and must be filled before actually delivering the message.
    // TODO: call shrink_to_fit from time to time
    messages_queue: VecDeque<nametbd_syscalls_interface::ffi::Message>,

    /// Interfaces that the process has registered.
    registered_interfaces: SmallVec<[[u8; 32]; 1]>,

    /// List of messages that the process has emitted and that are waiting for an answer.
    emitted_messages: SmallVec<[u64; 8]>,

    /// List of messages that the process is expected to answer.
    messages_to_answer: SmallVec<[u64; 8]>,
}

/// Additional information about a thread. Must be consistent with the actual state of the thread.
#[derive(Debug, PartialEq, Eq)]
enum Thread {
    /// Thread is ready to run.
    ReadyToRun,

    /// The thread is sleeping and waiting for a message to come.
    ///
    /// Note that this can be set even if the `messages_queue` is not empty, in the case where
    /// the thread is waiting only on messages that aren't in the queue.
    MessageWait(MessageWait),

    /// The thread wants to emit a message on an interface, but no handler was available.
    InterfaceNotAvailableWait {
        /// Interface we want to emit the message on.
        interface: [u8; 32],
        /// Identifier of the message if it expects an answer.
        message_id: Option<u64>,
        /// Message itself.
        message: Vec<u8>,
    },

    /// The thread is sleeping and waiting for an external extrinsic.
    ExtrinsicWait,

    /// Thread has been interrupted, and the call is being processed right now.
    InProcess,
}

/// How a process is waiting for messages.
#[derive(Debug, Clone, PartialEq, Eq)] // TODO: remove Clone
struct MessageWait {
    /// Identifiers of the messages we are waiting upon. Duplicate of what is in the process's
    /// memory.
    msg_ids: Vec<u64>,
    /// Offset within the memory of the process where the list of messages to wait upon is
    /// located. This is necessary as we have to zero.
    msg_ids_ptr: u32,
    /// Offset within the memory of the process where to write the received message.
    out_pointer: u32,
    /// Size of the memory of the process dedicated to receiving the message.
    out_size: u32,
}

/// Access to a process within the core.
pub struct CoreProcess<'a, T> {
    /// Access to the process within the inner collection.
    process: processes::ProcessesCollectionProc<'a, Process, Thread>,
    /// Marker to keep `T` in place.
    marker: PhantomData<T>,
}

/// Access to a thread within the core.
pub struct CoreThread<'a, T> {
    /// Access to the thread within the inner collection.
    thread: processes::ProcessesCollectionThread<'a, Process, Thread>,
    /// Marker to keep `T` in place.
    marker: PhantomData<T>,
}

impl<T: Clone> Core<T> {
    // TODO: figure out borrowing issues and remove that Clone
    /// Initialies a new `Core`.
    pub fn new() -> CoreBuilder<T> {
        CoreBuilder {
            interfaces: Default::default(),
            inner_builder: processes::ProcessesCollectionBuilder::default()
                .with_extrinsic(
                    "nametbd",
                    "next_message",
                    sig!((I32, I32, I32, I32, I32) -> I32),
                    Extrinsic::NextMessage,
                )
                .with_extrinsic(
                    "nametbd",
                    "emit_message",
                    sig!((I32, I32, I32, I32, I32) -> I32),
                    Extrinsic::EmitMessage,
                )
                .with_extrinsic(
                    "nametbd",
                    "emit_answer",
                    sig!((I32, I32, I32) -> I32),
                    Extrinsic::EmitAnswer,
                )
                .with_extrinsic(
                    "nametbd",
                    "cancel_message",
                    sig!((I32) -> I32),
                    Extrinsic::CancelMessage,
                ),
        }
    }

    /// Run the core once.
    // TODO: make multithreaded
    pub fn run(&mut self) -> CoreRunOutcome<T> {
        loop {
            break match self.run_inner() {
                CoreRunOutcomeInner::Idle => CoreRunOutcome::Idle,
                CoreRunOutcomeInner::LoopAgain => continue,
                CoreRunOutcomeInner::ProgramFinished {
                    process,
                    unhandled_messages,
                    cancelled_messages,
                    unregistered_interfaces,
                    outcome,
                } => CoreRunOutcome::ProgramFinished {
                    process,
                    unhandled_messages,
                    cancelled_messages,
                    unregistered_interfaces,
                    outcome,
                },
                CoreRunOutcomeInner::ThreadWaitExtrinsic {
                    thread,
                    extrinsic,
                    params,
                } => CoreRunOutcome::ThreadWaitExtrinsic {
                    thread: CoreThread {
                        thread: self.processes.thread_by_id(thread).unwrap(),
                        marker: PhantomData,
                    },
                    extrinsic,
                    params,
                },
                CoreRunOutcomeInner::ThreadWaitUnavailableInterface { thread, interface } => {
                    CoreRunOutcome::ThreadWaitUnavailableInterface {
                        thread: CoreThread {
                            thread: self.processes.thread_by_id(thread).unwrap(),
                            marker: PhantomData,
                        },
                        interface,
                    }
                }
                CoreRunOutcomeInner::InterfaceMessage {
                    pid,
                    message_id,
                    interface,
                    message,
                } => CoreRunOutcome::InterfaceMessage {
                    pid,
                    message_id,
                    interface,
                    message,
                },
                CoreRunOutcomeInner::MessageResponse {
                    pid,
                    message_id,
                    response,
                } => CoreRunOutcome::MessageResponse {
                    pid,
                    message_id,
                    response,
                },
            };
        }
    }

    /// Because of lifetime issues, we return an enum that holds `Pid`s instead of `CoreProcess`es.
    /// Then `run` does the conversion in order to have a good API.
    // TODO: make multithreaded
    fn run_inner(&mut self) -> CoreRunOutcomeInner<T> {
        match self.processes.run() {
            processes::RunOneOutcome::ProcessFinished {
                pid,
                outcome,
                dead_threads,
                user_data,
            } => {
                debug_assert_eq!(dead_threads[0].1, Thread::ReadyToRun);
                for (dead_thread_id, dead_thread_state) in dead_threads {
                    match dead_thread_state {
                        _ => {} // TODO:
                    }
                }

                // Unregister the interfaces this program had registered.
                let mut unregistered_interfaces = Vec::new();
                for interface in user_data.registered_interfaces {
                    let _interface = self.interfaces.remove(&interface);
                    debug_assert_eq!(_interface, Some(InterfaceHandler::Process(pid)));
                    unregistered_interfaces.push(interface);
                }

                // Cancelling messages that the process had emitted.
                // TODO: this only handles messages emitted through the external API
                let mut cancelled_messages = Vec::new();
                for emitted_message in user_data.emitted_messages {
                    let _emitter = self.messages_to_answer.remove(&emitted_message);
                    debug_assert_eq!(_emitter, Some(MessageEmitter::Process(pid)));
                    cancelled_messages.push(emitted_message);
                }

                // TODO: also, what do we do with the pending messages and all?

                CoreRunOutcomeInner::ProgramFinished {
                    process: pid,
                    unregistered_interfaces,
                    // TODO: this only handles messages emitted through the external API
                    unhandled_messages: user_data.messages_to_answer.to_vec(), // TODO: to_vec overhead
                    cancelled_messages,
                    outcome,
                }
            }

            processes::RunOneOutcome::ThreadFinished { user_data, .. } => {
                debug_assert_eq!(user_data, Thread::ReadyToRun);
                // TODO: report?
                CoreRunOutcomeInner::LoopAgain
            }

            processes::RunOneOutcome::Interrupted {
                ref mut thread,
                id: Extrinsic::External(ext),
                ref params,
            } => {
                debug_assert_eq!(*thread.user_data(), Thread::ReadyToRun);
                *thread.user_data() = Thread::ExtrinsicWait;
                CoreRunOutcomeInner::ThreadWaitExtrinsic {
                    thread: thread.tid(),
                    extrinsic: ext.clone(),
                    params: params.clone(), // TODO: there's some weird borrowck error in the match block
                }
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::NextMessage,
                params,
            } => {
                debug_assert_eq!(*thread.user_data(), Thread::ReadyToRun);
                *thread.user_data() = Thread::InProcess;
                // TODO: refactor a bit to first parse the parameters and then update `self`
                extrinsic_next_message(&mut thread, params);
                CoreRunOutcomeInner::LoopAgain
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::EmitMessage,
                params,
            } => {
                debug_assert_eq!(*thread.user_data(), Thread::ReadyToRun);
                *thread.user_data() = Thread::InProcess;

                // TODO: lots of unwraps here
                assert_eq!(params.len(), 5);
                let interface: [u8; 32] = {
                    let addr = params[0].try_into::<i32>().unwrap() as u32;
                    TryFrom::try_from(&thread.read_memory(addr, 32).unwrap()[..]).unwrap()
                };
                let message = {
                    let addr = params[1].try_into::<i32>().unwrap() as u32;
                    let sz = params[2].try_into::<i32>().unwrap() as u32;
                    thread.read_memory(addr, sz).unwrap()
                };
                let needs_answer = params[3].try_into::<i32>().unwrap() != 0;
                let emitter_pid = thread.pid();
                let message_id = if needs_answer {
                    let message_id_write = params[4].try_into::<i32>().unwrap() as u32;
                    let new_message_id = loop {
                        let id = self.message_id_pool.assign();
                        if id == 0 || id == 1 {
                            continue;
                        }
                        match self.messages_to_answer.entry(id) {
                            Entry::Occupied(_) => continue,
                            Entry::Vacant(e) => e.insert(MessageEmitter::Process(emitter_pid)),
                        };
                        break id;
                    };
                    let mut buf = [0; 8];
                    LittleEndian::write_u64(&mut buf, new_message_id);
                    thread.write_memory(message_id_write, &buf).unwrap();
                    // TODO: thread.user_data().;
                    // TODO: thread.process().user_data().emitted_messages.push();
                    Some(new_message_id)
                } else {
                    None
                };

                match self.interfaces.get(&interface) {
                    Some(InterfaceHandler::Process(pid)) => {
                        let message = nametbd_syscalls_interface::ffi::Message::Interface(
                            nametbd_syscalls_interface::ffi::InterfaceMessage {
                                interface,
                                index_in_list: 0,
                                message_id,
                                emitter_pid: Some(emitter_pid.into()),
                                actual_data: message,
                            },
                        );

                        *thread.user_data() = Thread::ReadyToRun;
                        thread.resume(Some(wasmi::RuntimeValue::I32(0)));
                        let mut process = self.processes.process_by_id(*pid).unwrap();
                        process.user_data().messages_queue.push_back(message);
                        CoreRunOutcomeInner::LoopAgain
                    }
                    Some(InterfaceHandler::External) => {
                        *thread.user_data() = Thread::ReadyToRun;
                        thread.resume(Some(wasmi::RuntimeValue::I32(0)));
                        CoreRunOutcomeInner::InterfaceMessage {
                            pid: thread.pid(),
                            message_id,
                            interface,
                            message,
                        }
                    }
                    None => {
                        // TODO: set to InterfaceNotAvailableWait instead
                        unimplemented!()
                    }
                }
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::EmitAnswer,
                params,
            } => {
                debug_assert_eq!(*thread.user_data(), Thread::ReadyToRun);
                *thread.user_data() = Thread::InProcess;

                // TODO: lots of unwraps here
                assert_eq!(params.len(), 3);
                let msg_id = {
                    let addr = params[0].try_into::<i32>().unwrap() as u32;
                    let buf = thread.read_memory(addr, 8).unwrap();
                    byteorder::LittleEndian::read_u64(&buf)
                };
                let message = {
                    let addr = params[1].try_into::<i32>().unwrap() as u32;
                    let sz = params[2].try_into::<i32>().unwrap() as u32;
                    thread.read_memory(addr, sz).unwrap()
                };
                let pid = thread.pid();
                drop(thread);
                self.answer_message_inner(msg_id, &message, Some(pid))
                    .unwrap_or(CoreRunOutcomeInner::LoopAgain)
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::CancelMessage,
                params,
            } => unimplemented!(),

            processes::RunOneOutcome::Idle => CoreRunOutcomeInner::Idle,
        }
    }

    /// Returns an object granting access to a process, if it exists.
    pub fn process_by_id(&mut self, pid: Pid) -> Option<CoreProcess<T>> {
        let p = self.processes.process_by_id(pid)?;
        Some(CoreProcess {
            process: p,
            marker: PhantomData,
        })
    }

    /// Returns an object granting access to a thread, if it exists.
    pub fn thread_by_id(&mut self, thread: ThreadId) -> Option<CoreThread<T>> {
        let thread = self.processes.thread_by_id(thread)?;
        Some(CoreThread {
            thread,
            marker: PhantomData,
        })
    }

    // TODO: better API
    pub fn set_interface_handler(&mut self, interface: [u8; 32], process: Pid) -> Result<(), ()> {
        if self.processes.process_by_id(process).is_none() {
            return Err(());
        }

        match self.interfaces.entry(interface) {
            Entry::Occupied(_) => return Err(()),
            Entry::Vacant(e) => e.insert(InterfaceHandler::Process(process)),
        };

        for thread_id in self
            .interface_waits
            .remove(&interface)
            .unwrap_or(SmallVec::new())
        {
            //let thread = self.processes.thread_by_id(thread_id);
            unimplemented!() // TODO:
        }

        Ok(())
    }

    /// Emits a message for the handler of the given interface.
    ///
    /// The message doesn't expect any answer.
    // TODO: better API
    pub fn emit_interface_message_no_answer(
        &mut self,
        interface: [u8; 32],
        message: impl Encode,
    ) -> Result<(), ()> {
        let message = nametbd_syscalls_interface::ffi::Message::Interface(
            nametbd_syscalls_interface::ffi::InterfaceMessage {
                interface,
                message_id: None,
                emitter_pid: None,
                index_in_list: 0,
                actual_data: message.encode(),
            },
        );

        let pid = match self.interfaces.get(&interface).ok_or(())? {
            InterfaceHandler::Process(pid) => *pid,
            InterfaceHandler::External => return Err(()), // TODO: explain that explicitely
        };

        let mut process = self.processes.process_by_id(pid).unwrap();
        process.user_data().messages_queue.push_back(message);

        try_resume_message_wait(process);
        Ok(())
    }

    /// Emits a message for the handler of the given interface.
    ///
    /// The message does expect an answer. The answer will be sent back as
    /// [`MessageResponse`](CoreRunOutcome::MessageResponse) event.
    // TODO: better API
    pub fn emit_interface_message_answer(
        &mut self,
        interface: [u8; 32],
        message: impl Encode,
    ) -> Result<u64, ()> {
        let (message_id, messages_to_answer_entry) = loop {
            let id = self.message_id_pool.assign();
            if id == 0 || id == 1 {
                continue;
            }
            match self.messages_to_answer.entry(id) {
                Entry::Vacant(e) => break (id, e),
                Entry::Occupied(_) => continue,
            };
        };

        let message = nametbd_syscalls_interface::ffi::Message::Interface(
            nametbd_syscalls_interface::ffi::InterfaceMessage {
                interface,
                message_id: Some(message_id),
                emitter_pid: None,
                index_in_list: 0,
                actual_data: message.encode(),
            },
        );

        let pid = match self.interfaces.get(&interface).ok_or_else(|| panic!())? {
            InterfaceHandler::Process(pid) => *pid,
            InterfaceHandler::External => panic!(),
        };

        let mut process = self.processes.process_by_id(pid).unwrap();
        process.user_data().messages_queue.push_back(message);
        try_resume_message_wait(process);
        messages_to_answer_entry.insert(MessageEmitter::External);
        Ok(message_id)
    }

    ///
    ///
    /// It is forbidden to answer messages created using [`emit_interface_message_answer`] or
    /// [`emit_interface_message_no_answer`]. Only messages generated by processes can be answered
    /// through this method.
    // TODO: better API
    pub fn answer_message(&mut self, message_id: u64, response: &[u8]) {
        let ret = self.answer_message_inner(message_id, response, None);
        assert!(ret.is_none());
    }

    // TODO: better API
    fn answer_message_inner(
        &mut self,
        message_id: u64,
        response: &[u8],
        answerer_pid: Option<Pid>,
    ) -> Option<CoreRunOutcomeInner<T>> {
        let actual_message = nametbd_syscalls_interface::ffi::Message::Response(
            nametbd_syscalls_interface::ffi::ResponseMessage {
                message_id,
                // We a dummy value here and fill it up later when actually delivering the message.
                index_in_list: 0,
                actual_data: response.to_vec(),
            },
        );

        match (self.messages_to_answer.remove(&message_id), answerer_pid) {
            (Some(MessageEmitter::Process(emitter_pid)), _) => {
                let mut process = self.processes.process_by_id(emitter_pid).unwrap();
                process.user_data().messages_queue.push_back(actual_message);
                process
                    .user_data()
                    .emitted_messages
                    .retain(|m| *m != message_id);
                try_resume_message_wait(process);
                None
            }
            (Some(MessageEmitter::External), Some(answerer_pid)) => {
                Some(CoreRunOutcomeInner::MessageResponse {
                    pid: answerer_pid,
                    message_id,
                    response: response.to_vec(),
                })
            }
            (None, _) | (Some(MessageEmitter::External), None) => {
                // TODO: what to do here?
                panic!("no process found with that event")
            }
        }
    }

    /// Start executing the module passed as parameter.
    ///
    /// Each import of the [`Module`](crate::module::Module) is resolved.
    pub fn execute(&mut self, module: &Module) -> Result<CoreProcess<T>, vm::NewErr> {
        let proc_metadata = Process {
            messages_queue: VecDeque::new(),
            registered_interfaces: SmallVec::new(),
            emitted_messages: SmallVec::new(),
            messages_to_answer: SmallVec::new(),
        };

        let process = self
            .processes
            .execute(module, proc_metadata, Thread::ReadyToRun)?;

        Ok(CoreProcess {
            process,
            marker: PhantomData,
        })
    }
}

impl<'a, T> CoreProcess<'a, T> {
    /// Returns the [`Pid`] of the process.
    pub fn pid(&self) -> Pid {
        self.process.pid()
    }

    /// Adds a new thread to the process, starting the function with the given index and passing
    /// the given parameters.
    // TODO: don't expose wasmi::RuntimeValue
    pub fn start_thread(
        self,
        fn_index: u32,
        params: Vec<wasmi::RuntimeValue>,
    ) -> Result<CoreThread<'a, T>, vm::StartErr> {
        let thread = self
            .process
            .start_thread(fn_index, params, Thread::ReadyToRun)?;
        Ok(CoreThread {
            thread,
            marker: PhantomData,
        })
    }

    /// Copies the given memory range of the given process into a `Vec<u8>`.
    ///
    /// Returns an error if the range is invalid.
    pub fn read_memory(&mut self, offset: u32, size: u32) -> Result<Vec<u8>, ()> {
        self.process.read_memory(offset, size)
    }

    pub fn write_memory(&mut self, offset: u32, data: &[u8]) -> Result<(), ()> {
        self.process.write_memory(offset, data)
    }

    /// Kills the process immediately.
    pub fn abort(self) {
        self.process.abort(); // TODO: clean up
    }
}

impl<'a, T> CoreThread<'a, T> {
    /// Returns the [`ThreadId`] of the thread.
    pub fn tid(&mut self) -> ThreadId {
        self.thread.tid()
    }

    /// Returns the [`Pid`] of the process associated to this thread.
    pub fn pid(&self) -> Pid {
        self.thread.pid()
    }

    /// After `ThreadWaitExtrinsic` has been returned, you have to call this method in order to
    /// inject back the result of the extrinsic call.
    // TODO: don't expose wasmi::RuntimeValue
    pub fn resolve_extrinsic_call(&mut self, return_value: Option<wasmi::RuntimeValue>) {
        assert_eq!(*self.thread.user_data(), Thread::ExtrinsicWait);
        *self.thread.user_data() = Thread::ReadyToRun;
        // TODO: check if the value type is correct
        self.thread.resume(return_value);
    }

    // TODO: resolve interface wait with error
}

impl<T> CoreBuilder<T> {
    /// Registers a function that processes can call.
    // TODO: more docs
    pub fn with_extrinsic(
        mut self,
        interface: impl Into<Cow<'static, str>>,
        f_name: impl Into<Cow<'static, str>>,
        signature: Signature,
        token: impl Into<T>,
    ) -> Self {
        self.inner_builder = self.inner_builder.with_extrinsic(
            interface,
            f_name,
            signature,
            Extrinsic::External(token.into()),
        );
        self
    }

    /// Marks the interface passed as parameter as "external".
    ///
    /// Messages destined to this interface will be returned in the [`CoreRunOutcome`] instead of
    /// being handled internally.
    ///
    /// # Panic
    ///
    /// Panics if this method has been previously called with the same interface.
    ///
    pub fn with_interface_handler(mut self, interface: impl Into<[u8; 32]>) -> Self {
        match self.interfaces.entry(interface.into()) {
            Entry::Occupied(_) => panic!(),
            Entry::Vacant(e) => e.insert(InterfaceHandler::External),
        };

        self
    }

    /// Turns the builder into a [`Core`].
    pub fn build(self) -> Core<T> {
        Core {
            processes: self.inner_builder.build(),
            interface_waits: HashMap::new(),
            interfaces: self.interfaces,
            message_id_pool: IdPool::new(),
            messages_to_answer: HashMap::default(),
        }
    }
}

/// Called when a thread calls the `next_message` extrinsic.
///
/// Tries to resume the thread by fetching a message from the queue.
///
/// Returns an error if the extrinsic call was invalid.
fn extrinsic_next_message(
    thread: &mut processes::ProcessesCollectionThread<Process, Thread>,
    params: Vec<wasmi::RuntimeValue>,
) -> Result<(), ()> {
    // TODO: lots of conversions here
    assert_eq!(params.len(), 5);

    let msg_ids_ptr = params[0].try_into::<i32>().ok_or(())? as u32;
    let msg_ids = {
        let addr = msg_ids_ptr;
        let len = params[1].try_into::<i32>().ok_or(())? as u32;
        let mem = thread.read_memory(addr, len * 8)?;
        let mut out = vec![0u64; len as usize];
        byteorder::LittleEndian::read_u64_into(&mem, &mut out);
        out
    };

    let out_pointer = params[2].try_into::<i32>().ok_or(())? as u32;
    let out_size = params[3].try_into::<i32>().ok_or(())? as u32;
    let block = params[4].try_into::<i32>().ok_or(())? != 0;

    assert!(*thread.user_data() == Thread::InProcess);
    *thread.user_data() = Thread::MessageWait(MessageWait {
        msg_ids,
        msg_ids_ptr,
        out_pointer,
        out_size,
    });

    try_resume_message_wait_thread(thread);

    // If `block` is false, we put the thread to sleep anyway, then wake it up again here.
    if !block && *thread.user_data() != Thread::ReadyToRun {
        debug_assert!(if let Thread::MessageWait(_) = thread.user_data() {
            true
        } else {
            false
        });
        *thread.user_data() = Thread::ReadyToRun;
        thread.resume(Some(wasmi::RuntimeValue::I32(0)));
    }

    Ok(())
}

/// If any of the threads of the given process is waiting for a message to arrive, checks the
/// queue and tries to resume said thread.
fn try_resume_message_wait(process: processes::ProcessesCollectionProc<Process, Thread>) {
    // TODO: is it a good strategy to just go through threads in linear order? what about
    //       round-robin-ness instead?
    let mut thread = process.main_thread();

    loop {
        try_resume_message_wait_thread(&mut thread);
        match thread.next_thread() {
            Some(t) => thread = t,
            None => break,
        };
    }
}

/// If the given thread is waiting for a message to arrive, checks the queue and tries to resume
/// said thread.
// TODO: in order to call this function, we essentially have to put the state machine in a "bad"
// state (message in queue and thread would accept said message); not great
fn try_resume_message_wait_thread(
    thread: &mut processes::ProcessesCollectionThread<Process, Thread>,
) {
    if thread.process_user_data().messages_queue.is_empty() {
        return;
    }

    let msg_wait = match thread.user_data() {
        Thread::MessageWait(ref wait) => wait.clone(), // TODO: don't clone?
        _ => return,
    };

    // Try to find a message in the queue that matches something the user is waiting for.
    let mut index_in_queue = 0;
    let index_in_msg_ids = loop {
        if index_in_queue >= thread.process_user_data().messages_queue.len() {
            // No message found.
            return;
        }

        // For that message in queue, grab the value that must be in `msg_ids` in order to match.
        let msg_id = match &thread.process_user_data().messages_queue[index_in_queue] {
            nametbd_syscalls_interface::ffi::Message::Interface(_) => 1,
            nametbd_syscalls_interface::ffi::Message::Response(response) => {
                debug_assert!(response.message_id >= 2);
                response.message_id
            }
        };

        if let Some(p) = msg_wait.msg_ids.iter().position(|id| *id == msg_id) {
            break p as u32;
        }

        index_in_queue += 1;
    };

    // If we reach here, we have found a message that matches what the user wants.

    // Adjust the `index_in_list` field of the message to match what we have.
    match thread.process_user_data().messages_queue[index_in_queue] {
        nametbd_syscalls_interface::ffi::Message::Response(ref mut response) => {
            response.index_in_list = index_in_msg_ids;
        }
        nametbd_syscalls_interface::ffi::Message::Interface(ref mut interface) => {
            interface.index_in_list = index_in_msg_ids;
        }
    }

    // Turn said message into bytes.
    // TODO: would be great to not do that every single time
    let msg_bytes = thread.process_user_data().messages_queue[index_in_queue].encode();

    // TODO: don't use as
    if msg_wait.out_size as usize >= msg_bytes.len() {
        // Write the message in the process's memory.
        thread
            .write_memory(msg_wait.out_pointer, &msg_bytes)
            .unwrap();
        // Zero the corresponding entry in the messages to wait upon.
        thread
            .write_memory(msg_wait.msg_ids_ptr + index_in_msg_ids * 8, &[0; 8])
            .unwrap();
        // Pop the message from the queue, so that we don't deliver it twice.
        thread
            .process_user_data()
            .messages_queue
            .remove(index_in_queue);
    }

    *thread.user_data() = Thread::ReadyToRun;
    thread.resume(Some(wasmi::RuntimeValue::I32(msg_bytes.len() as i32))); // TODO: don't use as
}

#[cfg(test)]
mod tests {
    use super::{Core, CoreRunOutcome};
    use crate::{
        module::Module,
        signature::{Signature, ValueType},
    };
    use core::iter;

    #[test]
    fn basic_module() {
        let module = Module::from_wat(
            r#"(module
            (func $_start (result i32)
                i32.const 5)
            (export "_start" (func $_start)))
        "#,
        )
        .unwrap();

        let mut core = Core::<()>::new().build();
        let expected_pid = core.execute(&module).unwrap().pid();

        match core.run() {
            CoreRunOutcome::ProgramFinished {
                process,
                outcome: Ok(ret_val),
                ..
            } => {
                assert_eq!(process, expected_pid);
                assert_eq!(ret_val, Some(wasmi::RuntimeValue::I32(5)));
            }
            _ => panic!(),
        }
    }

    #[test]
    #[ignore] // TODO: test fails
    fn trapping_module() {
        let module = Module::from_wat(
            r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                unreachable)
            (export "main" (func $main)))
        "#,
        )
        .unwrap();

        let mut core = Core::<()>::new().build();
        let expected_pid = core.execute(&module).unwrap().pid();

        match core.run() {
            CoreRunOutcome::ProgramFinished {
                process,
                outcome: Err(_),
                ..
            } => {
                assert_eq!(process, expected_pid);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn module_wait_extrinsic() {
        let module = Module::from_wat(
            r#"(module
            (import "foo" "test" (func $test (result i32)))
            (func $_start (result i32)
                call $test)
            (export "_start" (func $_start)))
        "#,
        )
        .unwrap();

        let mut core = Core::<u32>::new()
            .with_extrinsic(
                "foo",
                "test",
                Signature::new(iter::empty(), Some(ValueType::I32)),
                639u32,
            )
            .build();

        let expected_pid = core.execute(&module).unwrap().pid();

        let thread_id = match core.run() {
            CoreRunOutcome::ThreadWaitExtrinsic {
                mut thread,
                extrinsic,
                params,
            } => {
                assert_eq!(thread.pid(), expected_pid);
                assert_eq!(extrinsic, 639);
                assert!(params.is_empty());
                thread.tid()
            }
            _ => panic!(),
        };

        core.thread_by_id(thread_id)
            .unwrap()
            .resolve_extrinsic_call(Some(wasmi::RuntimeValue::I32(713)));

        match core.run() {
            CoreRunOutcome::ProgramFinished {
                process,
                outcome: Ok(ret_val),
                ..
            } => {
                assert_eq!(process, expected_pid);
                assert_eq!(ret_val, Some(wasmi::RuntimeValue::I32(713)));
            }
            _ => panic!(),
        }
    }

    #[test]
    #[should_panic]
    fn duplicate_interface_handler() {
        let interface: [u8; 32] = [4; 32];
        Core::<()>::new()
            .with_interface_handler(interface)
            .with_interface_handler(interface);
    }
}
