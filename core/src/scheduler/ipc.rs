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

use crate::id_pool::IdPool;
use crate::module::Module;
use crate::scheduler::{
    extrinsics::{self, ProcessesCollectionExtrinsicsThreadAccess as _},
    vm,
};
use crate::InterfaceHash;

use alloc::{collections::VecDeque, vec::Vec};
use core::{cell::RefCell, convert::TryFrom, iter, mem};
use crossbeam_queue::SegQueue;
use fnv::FnvBuildHasher;
use hashbrown::{hash_map::Entry, HashMap, HashSet};
use nohash_hasher::BuildNoHashHasher;
use redshirt_syscalls::{Encode, EncodedMessage, MessageId, Pid, ThreadId};
use smallvec::SmallVec;

/// Handles scheduling processes and inter-process communications.
pub struct Core {
    /// Queue of events to return in priority when `run` is called.
    pending_events: SegQueue<CoreRunOutcome>,

    /// List of running processes.
    processes: extrinsics::ProcessesCollectionExtrinsics<RefCell<Process>, ()>,

    /// List of `Pid`s that have been reserved during the construction.
    ///
    /// Never modified after initialization.
    reserved_pids: HashSet<Pid, BuildNoHashHasher<u64>>,

    /// For each interface, which program is fulfilling it.
    interfaces: RefCell<HashMap<InterfaceHash, InterfaceState, FnvBuildHasher>>,

    /// Pool of identifiers for messages.
    message_id_pool: IdPool,

    /// List of messages that have been emitted by a process and that are waiting for a response.
    // TODO: doc about hash safety
    // TODO: call shrink_to from time to time
    messages_to_answer: RefCell<HashMap<MessageId, Pid, BuildNoHashHasher<u64>>>,
}

/// Which way an interface is handled.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InterfaceState {
    /// Interface has been registered using [`Core::set_interface_handler`].
    Process(Pid),
    /// Interface hasn't been registered yet, but has been requested.
    Requested {
        /// List of threads waiting for this interface. All the threads in this list must be in
        /// the [`Thread::InterfaceNotAvailableWait`] state.
        threads: SmallVec<[ThreadId; 4]>,
        /// Other messages waiting to be delivered to this interface.
        other: Vec<(Pid, Option<MessageId>, EncodedMessage)>,
    },
}

/// Prototype for a `Core` under construction.
pub struct CoreBuilder {
    /// See the corresponding field in `Core`.
    reserved_pids: HashSet<Pid, BuildNoHashHasher<u64>>,
    /// Builder for the [`processes`][Core::processes] field in `Core`.
    inner_builder: extrinsics::ProcessesCollectionExtrinsicsBuilder,
}

/// Outcome of calling [`run`](Core::run).
// TODO: #[derive(Debug)]
pub enum CoreRunOutcome {
    /// A program has stopped, either because the main function has stopped or a problem has
    /// occurred.
    ProgramFinished {
        /// Id of the program that has stopped.
        pid: Pid,

        /// List of messages emitted using [`Core::emit_interface_message_answer`] that were
        /// supposed to be handled by the process that has just terminated.
        unhandled_messages: Vec<MessageId>,

        /// List of messages for which a [`CoreRunOutcome::InterfaceMessage`] has been emitted
        /// but that no loner need answering.
        cancelled_messages: Vec<MessageId>,

        /// List of interfaces that were registered by th process and no longer are.
        unregistered_interfaces: Vec<InterfaceHash>,

        /// How the program ended. If `Ok`, it has gracefully terminated. If `Err`, something
        /// bad happened.
        // TODO: force Ok to i32?
        outcome: Result<Option<wasmi::RuntimeValue>, wasmi::Trap>,
    },

    /// Thread has tried to emit a message on an interface that isn't registered. The thread is
    /// now in sleep mode. You can either wake it up by calling [`set_interface_handler`], or
    /// resume the thread with an "interface not available error" by calling . // TODO
    ThreadWaitUnavailableInterface {
        /// Thread that emitted the message.
        thread_id: ThreadId,

        /// Interface that the thread is trying to access.
        interface: InterfaceHash,
    },

    /// A process has emitted a message on an interface registered with a reserved PID.
    ReservedPidInterfaceMessage {
        pid: Pid,
        message_id: Option<MessageId>,
        interface: InterfaceHash,
        message: EncodedMessage,
    },

    /// Response to a message emitted using [`Core::emit_interface_message_answer`].
    MessageResponse {
        message_id: MessageId,
        response: Result<EncodedMessage, ()>,
    },

    /// Nothing to do. No thread is ready to run.
    Idle,
}

/// Additional information about a process.
#[derive(Debug)]
struct Process {
    /// Notifications available for retrieval by the process by calling `next_notification`.
    ///
    /// Note that the [`ResponseNotification::index_in_list`](redshirt_syscalls::ffi::ResponseNotification::index_in_list)
    /// and [`InterfaceMessage::index_in_list`](redshirt_syscalls::ffi::InterfaceMessage::index_in_list) fields are
    /// set to a dummy value, and must be filled before actually delivering the notification.
    // TODO: call shrink_to_fit from time to time
    notifications_queue: VecDeque<redshirt_syscalls::ffi::NotificationBuilder>,

    /// Interfaces that the process has registered.
    registered_interfaces: SmallVec<[InterfaceHash; 1]>,

    /// List of interfaces that this process has used. When the process dies, we notify all the
    /// handlers about it.
    used_interfaces: HashSet<InterfaceHash, FnvBuildHasher>,

    /// List of messages that the process has emitted and that are waiting for an answer.
    emitted_messages: SmallVec<[MessageId; 8]>,

    /// List of messages that the process is expected to answer.
    messages_to_answer: SmallVec<[MessageId; 8]>,
}

/// Access to a process within the core.
pub struct CoreProcess<'a> {
    /// Access to the process within the inner collection.
    process: extrinsics::ProcessesCollectionExtrinsicsProc<'a, RefCell<Process>, ()>,
}

impl Core {
    /// Initialies a new `Core`.
    pub fn new() -> CoreBuilder {
        CoreBuilder {
            reserved_pids: HashSet::with_hasher(Default::default()),
            inner_builder: extrinsics::ProcessesCollectionExtrinsicsBuilder::default(),
        }
    }

    /// Run the core once.
    pub fn run(&self) -> CoreRunOutcome {
        loop {
            match self.run_inner() {
                Some(ev) => break ev,
                None => {}
            }
        }
    }

    /// Same as [`run`]. Returns `None` if no event should be returned and we should loop again.
    fn run_inner(&self) -> Option<CoreRunOutcome> {
        if let Ok(ev) = self.pending_events.pop() {
            return Some(ev);
        }

        // Note: we use a temporary `run_outcome` variable in order to solve weird borrowing
        // issues. Feel free to try to remove it if you manage.
        let run_outcome = self.processes.run();
        match run_outcome {
            extrinsics::RunOneOutcome::ProcessFinished {
                pid,
                outcome,
                dead_threads,
                user_data,
            } => {
                for (dead_thread_id, dead_thread_state) in dead_threads {
                    match dead_thread_state {
                        _ => {} // TODO:
                    }
                }

                let user_data = user_data.into_inner();

                // Unregister the interfaces this program had registered.
                let mut unregistered_interfaces = Vec::new();
                for interface in user_data.registered_interfaces {
                    let _interface = self.interfaces.borrow_mut().remove(&interface);
                    debug_assert_eq!(_interface, Some(InterfaceState::Process(pid)));
                    unregistered_interfaces.push(interface);
                }

                // Cancelling messages that the process had emitted.
                // TODO: this only handles messages emitted through the external API
                let mut cancelled_messages = Vec::new();
                for emitted_message in user_data.emitted_messages {
                    let _emitter = self
                        .messages_to_answer
                        .borrow_mut()
                        .remove(&emitted_message);
                    debug_assert_eq!(_emitter, Some(pid));
                    cancelled_messages.push(emitted_message);
                }

                // Notify interface handlers about the process stopping.
                for interface in user_data.used_interfaces {
                    match self.interfaces.borrow().get(&interface) {
                        Some(InterfaceState::Process(p)) => {
                            if let Some(process) = self.processes.process_by_id(*p) {
                                let notif = From::from(
                                    redshirt_syscalls::ffi::build_process_destroyed_notification(
                                        pid.into(),
                                        0,
                                    ),
                                );

                                process
                                    .user_data()
                                    .borrow_mut()
                                    .notifications_queue
                                    .push_back(notif);
                                try_resume_notification_wait(process);
                            } // TODO: notify externals as well?
                        }
                        None => unreachable!(),
                        _ => {}
                    }
                }

                // TODO: also, what do we do with the pending messages and all?

                Some(CoreRunOutcome::ProgramFinished {
                    pid,
                    unregistered_interfaces,
                    // TODO: this only handles messages emitted through the external API
                    unhandled_messages: user_data.messages_to_answer.to_vec(), // TODO: to_vec overhead
                    cancelled_messages,
                    outcome,
                })
            }

            extrinsics::RunOneOutcome::ThreadFinished { .. } => {
                // TODO: report?
                None
            }

            extrinsics::RunOneOutcome::ThreadWaitNotification(thread) => {
                try_resume_notification_wait_thread(thread);
                None
            }

            extrinsics::RunOneOutcome::ThreadEmitMessage(mut thread) => {
                let emitter_pid = thread.pid();
                let interface = thread.emit_interface().clone();
                thread
                    .process_user_data()
                    .borrow_mut()
                    .used_interfaces
                    .insert(interface.clone());

                let mut self_interfaces_borrow = self.interfaces.borrow_mut();
                match (
                    self_interfaces_borrow.get_mut(&interface),
                    thread.allow_delay(),
                ) {
                    (Some(InterfaceState::Process(pid)), _) => {
                        let message_id = if thread.needs_answer() {
                            Some(loop {
                                let id: MessageId = self.message_id_pool.assign();
                                if u64::from(id) == 0 || u64::from(id) == 1 {
                                    continue;
                                }
                                match self.messages_to_answer.borrow_mut().entry(id) {
                                    Entry::Occupied(_) => continue,
                                    Entry::Vacant(e) => e.insert(emitter_pid),
                                };
                                break id;
                            })
                        } else {
                            None
                        };

                        let message = thread.accept_emit(message_id);
                        if let Some(process) = self.processes.process_by_id(*pid) {
                            let notif = redshirt_syscalls::ffi::build_interface_notification(
                                &interface,
                                message_id,
                                emitter_pid,
                                0,
                                &message,
                            )
                            .into();

                            process
                                .user_data()
                                .borrow_mut()
                                .notifications_queue
                                .push_back(notif);
                            try_resume_notification_wait(process);
                            None
                        } else if self.reserved_pids.contains(pid) {
                            Some(CoreRunOutcome::ReservedPidInterfaceMessage {
                                pid: emitter_pid,
                                message_id,
                                interface,
                                message,
                            })
                        } else {
                            // This can be reached if a process has been killed but the list of
                            // interface handlers hasn't been updated yet.
                            // TODO: this is wrong; don't just ignore the message
                            None
                        }
                    }
                    (None, false) | (Some(InterfaceState::Requested { .. }), false) => {
                        thread.refuse_emit();
                        None
                    }
                    (Some(InterfaceState::Requested { threads, .. }), true) => {
                        threads.push(thread.tid());
                        Some(CoreRunOutcome::ThreadWaitUnavailableInterface {
                            thread_id: thread.tid(),
                            interface,
                        })
                    }
                    (None, true) => {
                        self_interfaces_borrow.insert(
                            interface.clone(),
                            InterfaceState::Requested {
                                threads: iter::once(thread.tid()).collect(),
                                other: Vec::new(),
                            },
                        );
                        Some(CoreRunOutcome::ThreadWaitUnavailableInterface {
                            thread_id: thread.tid(),
                            interface,
                        })
                    }
                }
            }

            extrinsics::RunOneOutcome::ThreadEmitAnswer {
                message_id,
                ref response,
                ..
            } => {
                // TODO: check ownership of the message
                let response = response.clone();
                drop(run_outcome);
                self.answer_message_inner(message_id, Ok(response))
            }

            extrinsics::RunOneOutcome::ThreadEmitMessageError { message_id, .. } => {
                // TODO: check ownership of the message
                drop(run_outcome);
                self.answer_message_inner(message_id, Err(()))
            }

            extrinsics::RunOneOutcome::ThreadCancelMessage { message_id, .. } => {
                // TODO: check ownership of the message
                drop(run_outcome);
                self.messages_to_answer.borrow_mut().remove(&message_id);
                None
            }

            extrinsics::RunOneOutcome::Idle => Some(CoreRunOutcome::Idle),
        }
    }

    /// Returns an object granting access to a process, if it exists.
    pub fn process_by_id(&self, pid: Pid) -> Option<CoreProcess> {
        let p = self.processes.process_by_id(pid)?;
        Some(CoreProcess { process: p })
    }

    // TODO: better API
    pub fn set_interface_handler(&self, interface: InterfaceHash, process: Pid) -> Result<(), ()> {
        if self.processes.process_by_id(process).is_none() {
            if !self.reserved_pids.contains(&process) {
                return Err(());
            }
        } else {
            debug_assert!(!self.reserved_pids.contains(&process));
        }

        let (thread_ids, other_messages) =
            match self.interfaces.borrow_mut().entry(interface.clone()) {
                Entry::Vacant(e) => {
                    e.insert(InterfaceState::Process(process));
                    return Ok(());
                }
                Entry::Occupied(mut e) => {
                    // Check whether interface was already registered.
                    if let InterfaceState::Requested { .. } = *e.get_mut() {
                    } else {
                        return Err(());
                    };
                    match mem::replace(e.get_mut(), InterfaceState::Process(process)) {
                        InterfaceState::Requested { threads, other } => (threads, other),
                        _ => unreachable!(),
                    }
                }
            };

        // Send the `other_messages`.
        // TODO: should we preserve the order w.r.t. `threads`?
        for (emitter_pid, message_id, message_data) in other_messages {
            let notif = From::from(redshirt_syscalls::ffi::build_interface_notification(
                &interface,
                message_id,
                emitter_pid,
                0,
                &message_data,
            ));

            match self.processes.process_by_id(process) {
                Some(p) => p
                    .user_data()
                    .borrow_mut()
                    .notifications_queue
                    .push_back(notif),
                None => unreachable!(),
            }
        }

        // Now process the threads that were waiting for this interface to be registered.
        for thread_id in thread_ids {
            let mut thread = match self.processes.interrupted_thread_by_id(thread_id) {
                Ok(extrinsics::ProcessesCollectionExtrinsicsThread::EmitMessage(t)) => t,
                _ => unreachable!(),
            };

            debug_assert_eq!(thread.emit_interface(), interface);
            let emitter_pid = thread.pid().into();

            let message_id = if thread.needs_answer() {
                Some(loop {
                    let id: MessageId = self.message_id_pool.assign();
                    if u64::from(id) == 0 || u64::from(id) == 1 {
                        continue;
                    }
                    match self.messages_to_answer.borrow_mut().entry(id) {
                        Entry::Occupied(_) => continue,
                        Entry::Vacant(e) => e.insert(emitter_pid),
                    };
                    break id;
                })
            } else {
                None
            };

            let message = thread.accept_emit(message_id);

            if let Some(interface_handler_proc) = self.processes.process_by_id(process) {
                let notif = From::from(redshirt_syscalls::ffi::build_interface_notification(
                    &interface,
                    message_id,
                    emitter_pid,
                    0,
                    &message,
                ));

                interface_handler_proc
                    .user_data()
                    .borrow_mut()
                    .notifications_queue
                    .push_back(notif);
            } else {
                debug_assert!(self.reserved_pids.contains(&process));
                self.pending_events
                    .push(CoreRunOutcome::ReservedPidInterfaceMessage {
                        pid: emitter_pid,
                        message_id,
                        interface: interface.clone(),
                        message,
                    });
            }
        }

        if let Some(interface_handler_proc) = self.processes.process_by_id(process) {
            try_resume_notification_wait(interface_handler_proc);
        }

        Ok(())
    }

    /// Emits a message for the handler of the given interface.
    ///
    /// The message doesn't expect any answer.
    // TODO: better API
    pub fn emit_interface_message_no_answer<'a>(
        &self,
        emitter_pid: Pid,
        interface: InterfaceHash,
        message: impl Encode,
    ) {
        assert!(self.reserved_pids.contains(&emitter_pid));
        let _out = self.emit_interface_message_inner(emitter_pid, interface, message, false);
        debug_assert!(_out.is_none());
    }

    /// Emits a message for the handler of the given interface.
    ///
    /// The message does expect an answer. The answer will be sent back as
    /// [`MessageResponse`](CoreRunOutcome::MessageResponse) event.
    // TODO: better API
    pub fn emit_interface_message_answer<'a>(
        &self,
        emitter_pid: Pid,
        interface: InterfaceHash,
        message: impl Encode,
    ) -> MessageId {
        assert!(self.reserved_pids.contains(&emitter_pid));
        match self.emit_interface_message_inner(emitter_pid, interface, message, true) {
            Some(m) => m,
            None => unreachable!(),
        }
    }

    fn emit_interface_message_inner<'a>(
        &self,
        emitter_pid: Pid,
        interface: InterfaceHash,
        message: impl Encode,
        needs_answer: bool,
    ) -> Option<MessageId> {
        let mut messages_to_answer = self.messages_to_answer.borrow_mut();

        let (message_id, messages_to_answer_entry) = if needs_answer {
            loop {
                let id: MessageId = self.message_id_pool.assign();
                if u64::from(id) == 0 || u64::from(id) == 1 {
                    continue;
                }
                match messages_to_answer.entry(id) {
                    Entry::Vacant(e) => break (Some(id), Some(e)),
                    Entry::Occupied(_) => continue,
                };
            }
        } else {
            (None, None)
        };

        let pid = match self
            .interfaces
            .borrow_mut()
            .entry(interface.clone())
            .or_insert_with(|| InterfaceState::Requested {
                threads: SmallVec::new(),
                other: Vec::new(),
            }) {
            InterfaceState::Process(pid) => *pid,
            InterfaceState::Requested { other, .. } => {
                other.push((emitter_pid, message_id, message.encode()));
                return message_id;
            }
        };

        if let Some(process) = self.processes.process_by_id(pid) {
            let notif = redshirt_syscalls::ffi::build_interface_notification(
                &interface,
                message_id,
                emitter_pid,
                0,
                &message.encode(),
            );

            process
                .user_data()
                .borrow_mut()
                .notifications_queue
                .push_back(From::from(notif));
            try_resume_notification_wait(process);
        } else if self.reserved_pids.contains(&emitter_pid) {
            self.pending_events
                .push(CoreRunOutcome::ReservedPidInterfaceMessage {
                    pid: emitter_pid,
                    message_id: None,
                    interface,
                    message: message.encode(),
                });
        } else {
            unimplemented!()
        };

        if let Some(messages_to_answer_entry) = messages_to_answer_entry {
            messages_to_answer_entry.insert(emitter_pid);
        }
        message_id
    }

    ///
    ///
    /// It is forbidden to answer messages created using [`emit_interface_message_answer`] or
    /// [`emit_interface_message_no_answer`]. Only messages generated by processes can be answered
    /// through this method.
    // TODO: better API
    pub fn answer_message(&self, message_id: MessageId, response: Result<EncodedMessage, ()>) {
        let ret = self.answer_message_inner(message_id, response);
        //assert!(ret.is_none());
    }

    // TODO: better API
    fn answer_message_inner(
        &self,
        message_id: MessageId,
        response: Result<EncodedMessage, ()>,
    ) -> Option<CoreRunOutcome> {
        if let Some(emitter_pid) = self.messages_to_answer.borrow_mut().remove(&message_id) {
            if let Some(process) = self.processes.process_by_id(emitter_pid) {
                let notif = From::from(redshirt_syscalls::ffi::build_response_notification(
                    message_id,
                    // We a dummy value here and fill it up later when actually delivering the notif.
                    0,
                    match &response {
                        Ok(r) => Ok(r),
                        Err(()) => Err(()),
                    },
                ));

                process
                    .user_data()
                    .borrow_mut()
                    .notifications_queue
                    .push_back(notif);
                process
                    .user_data()
                    .borrow_mut()
                    .emitted_messages
                    .retain(|m| *m != message_id);
                try_resume_notification_wait(process);
                None
            } else {
                Some(CoreRunOutcome::MessageResponse {
                    message_id,
                    response,
                })
            }
        } else {
            // TODO: this can happen if message was cancelled
            // TODO: figure this out more properly?
            None
        }
    }

    /// Start executing the module passed as parameter.
    ///
    /// Each import of the [`Module`](crate::module::Module) is resolved.
    pub fn execute(&self, module: &Module) -> Result<CoreProcess, vm::NewErr> {
        let proc_metadata = Process {
            notifications_queue: VecDeque::new(),
            registered_interfaces: SmallVec::new(),
            used_interfaces: HashSet::with_hasher(Default::default()),
            emitted_messages: SmallVec::new(),
            messages_to_answer: SmallVec::new(),
        };

        let process = self
            .processes
            .execute(module, RefCell::new(proc_metadata), ())?;

        Ok(CoreProcess { process })
    }
}

impl<'a> CoreProcess<'a> {
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
    ) -> Result<(), vm::StartErr> {
        self.process.start_thread(fn_index, params, ())?;
        Ok(())
    }

    /// Kills the process immediately.
    pub fn abort(&self) {
        self.process.abort(); // TODO: clean up
    }
}

impl CoreBuilder {
    /// Allocates a `Pid` that will not be used by any process.
    ///
    /// > **Note**: As of the writing of this comment, this feature is only ever used to allocate
    /// >           `Pid`s that last forever. There is therefore no corresponding "unreserve_pid"
    /// >           method that frees such an allocated `Pid`. If there is ever a need to free
    /// >           these `Pid`s, such a method should be added.
    pub fn reserve_pid(&mut self) -> Pid {
        let pid = self.inner_builder.reserve_pid();
        let _was_inserted = self.reserved_pids.insert(pid);
        debug_assert!(_was_inserted);
        pid
    }

    /// Turns the builder into a [`Core`].
    pub fn build(mut self) -> Core {
        self.reserved_pids.shrink_to_fit();

        Core {
            pending_events: SegQueue::new(),
            processes: self.inner_builder.build(),
            interfaces: RefCell::new(Default::default()),
            reserved_pids: self.reserved_pids,
            message_id_pool: IdPool::new(),
            messages_to_answer: RefCell::new(HashMap::default()),
        }
    }
}

/// If any of the threads of the given process is waiting for a message to arrive, checks the
/// queue and tries to resume said thread.
fn try_resume_notification_wait(
    process: extrinsics::ProcessesCollectionExtrinsicsProc<RefCell<Process>, ()>,
) {
    // TODO: is it a good strategy to just go through threads in linear order? what about
    //       round-robin-ness instead?
    for thread in process.interrupted_threads() {
        if let extrinsics::ProcessesCollectionExtrinsicsThread::WaitNotification(t) = thread {
            try_resume_notification_wait_thread(t)
        }
    }
}

/// If the given thread is waiting for a notification to arrive, checks the queue and tries to
/// resume said thread.
// TODO: in order to call this function, we essentially have to put the state machine in a "bad"
// state (notifications in queue and thread would accept said notification); not great
fn try_resume_notification_wait_thread(
    mut thread: extrinsics::ProcessesCollectionExtrinsicsThreadWaitNotification<
        RefCell<Process>,
        (),
    >,
) {
    // Try to find a notification in the queue that matches something the user is waiting for.
    let mut index_in_queue = 0;
    let index_in_msg_ids = loop {
        if index_in_queue
            >= thread
                .process_user_data()
                .borrow_mut()
                .notifications_queue
                .len()
        {
            // No notification found.
            if !thread.block() {
                thread.resume_no_notification();
            }
            return;
        }

        // For that notification in queue, grab the value that must be in `msg_ids` in order to match.
        let msg_id = match &thread.process_user_data().borrow_mut().notifications_queue
            [index_in_queue]
        {
            redshirt_syscalls::ffi::NotificationBuilder::Interface(_) => MessageId::from(1),
            redshirt_syscalls::ffi::NotificationBuilder::ProcessDestroyed(_) => MessageId::from(1),
            redshirt_syscalls::ffi::NotificationBuilder::Response(response) => {
                debug_assert!(u64::from(response.message_id()) >= 2);
                response.message_id()
            }
        };

        if let Some(p) = thread.message_ids_iter().position(|id| id == msg_id.into()) {
            break p;
        }

        index_in_queue += 1;
    };

    // If we reach here, we have found a notification that matches what the user wants.

    let notif_length =
        thread.process_user_data().borrow_mut().notifications_queue[index_in_queue].len();

    // TODO: maybe extrinsics could have some API shortcut here
    if notif_length <= thread.allowed_notification_size() {
        // Pop the notification from the queue, so that we don't deliver it twice.
        let mut notification = thread
            .process_user_data()
            .borrow_mut()
            .notifications_queue
            .remove(index_in_queue)
            .unwrap();

        // Adjust the `index_in_list` field of the notification to match what we have.
        notification.set_index_in_list(u32::try_from(index_in_msg_ids).unwrap());
        // TODO: crappy to pass an EncodedMessage
        thread.resume_notification(index_in_msg_ids, EncodedMessage(notification.into_bytes()))
    } else {
        thread.resume_notification_too_big(notif_length)
    }
}
