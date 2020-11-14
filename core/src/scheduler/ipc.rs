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

use crate::{
    extrinsics::Extrinsics,
    id_pool::IdPool,
    module::Module,
    scheduler::{
        extrinsics::{self, ThreadAccessAccess as _},
        vm,
    },
    InterfaceHash,
};

use alloc::vec::Vec;
use crossbeam_queue::SegQueue;
use fnv::FnvBuildHasher;
use hashbrown::{hash_map::Entry, HashMap, HashSet};
use nohash_hasher::BuildNoHashHasher;
use redshirt_syscalls::{Encode, EncodedMessage, MessageId, Pid, ThreadId};
use smallvec::SmallVec;
use spinning_top::Spinlock;

mod notifications_queue;
mod waiting_threads;

/// Handles scheduling processes and inter-process communications.
///
/// # State of messages
///
/// The possible states of a message are as follow:
///
/// - Generated by a program but not accepted yet. A [`CoreRunOutcome::InterfaceMessage`] is
///   emitted. The thread that has generated the message is sleeping. If the message has the
///   "immediate-delivery" flag on, it can then be refused by calling
///   [`Core::reject_immediate_interface_message`]. The emitting thread is resumed with an error.
///
/// - Accepted "internally" by calling [`Core::accept_interface_message_answerer`] on a
///   not-accepted-yet message that expects an answer (in which case the thread that has emitted
///   the message is resumed), or by calling [`Core::allocate_message_answerer`]. The process
///   passed as parameter to either method is later responsible for answering that message. In the
///   case of [`Core::allocate_message_answerer`], this answer is delivered through a
///   [`CoreRunOutcome::AnsweredMessage`].
///
/// - Accepted "externally" by calling [`Core::accept_interface_message`] on a not-accepted-yet
///   message. The thread that has emitted the message is resumed, and, if the message expects an
///   answer, the user is responsible for later answering the message with
///   [`Core::answer_message`].
///
/// Note that when a program emits a message that doesn't need an answer, this message is assigned
/// a [`MessageId`] for API-related purposes. This [`MessageId`] isn't expected to ever reach a
/// program's user space. As soon as the message is accepted or refused, the [`MessageId`] is
/// discarded.
///
/// A [`Core::allocate_untracked_message`] method is also provided in order to avoid accidental
/// collisions, but the [`MessageId`]s allocated through it don't have a tracked state.
///
// # Implementation notes
//
// This struct synchronizes the following components in a lock-free way:
//
// - The underlying VMs, with processes and threads.
// - For each process, a list of answers waiting to be delivered.
// - For each process, a list of threads blocked waiting for answers and that we have failed to
//   resume in the past.
// - A list of active messages waiting to be answered.
//
// While each of these components is updated atomically, there exists no synchronization between
// them. As such, the implementation heavily relies on the fact that message IDs, process IDs,
// and thread IDs are unique.
//
// For example, delivering a message to an interface consists in atomically looking for the process
// that handles this interface, then atomically delivering it. If, in parallel, that process has
// terminated but has not yet been unregistered as the handler of the interface, then we know it
// from the fact that he process ID is no longer valid. This wouldn't be possible if process IDs
// were reused.
//
// TODO: finish updating this section ^
pub struct Core<TExt: Extrinsics> {
    /// Pool of identifiers where `MessageId`s are allocated.
    id_pool: IdPool,

    /// Queue of events to return in priority when `run` is called.
    pending_events: SegQueue<CoreRunOutcome>,

    /// List of running processes.
    processes: extrinsics::ProcessesCollectionExtrinsics<Process, (), TExt>,

    /// List of messages that have been emitted by a thread but haven't been accepted or refused
    /// yet. Stores the emitter of the message.
    pending_accept_messages:
        Spinlock<HashMap<MessageId, (Pid, ThreadId), nohash_hasher::BuildNoHashHasher<u64>>>,

    /// List of messages that have been emitted by a process but haven't been answered yet. Stores
    /// the emitter of the message.
    pending_answer_messages:
        Spinlock<HashMap<MessageId, Pid, nohash_hasher::BuildNoHashHasher<u64>>>,
}

/// Prototype for a `Core` under construction.
pub struct CoreBuilder<TExt: Extrinsics> {
    /// Builder for the [`processes`][Core::processes] field in [`Core`].
    inner_builder: extrinsics::Builder<TExt>,
}

/// Outcome of calling [`run`](Core::run).
#[derive(Debug)]
pub enum CoreRunOutcome {
    /// A program has stopped, either because the main function has stopped or a problem has
    /// occurred.
    ProgramFinished {
        /// Id of the program that has stopped.
        pid: Pid,

        /// List of messages allocated using [`Core::allocate_message_answerer`] that the process
        /// was responsible for answering.
        ///
        /// One should treat the messages in this list as if a [`CoreRunOutcome::AnsweredMessage`]
        /// with `answer` equal to `Err(())` had been emitted for each of them.
        ///
        /// > **Note**: Messages passed to [`Core::accept_interface_message_answerer`] are *not*
        /// >           in this list. The emitter of the message is directly informed of the
        /// >           message failing.
        unanswered_messages: Vec<MessageId>,

        /// How the program ended. If `Ok`, it has gracefully terminated. If `Err`, something
        /// bad happened.
        // TODO: force Ok to i32?
        // TODO: don't expose wasmi in error
        outcome: Result<Option<crate::WasmValue>, wasmi::Trap>,
    },

    /// A process wants to emit a message on an interface.
    ///
    /// If the `needs_answer` is `true`, you must call
    /// [`CoreRunOutcome::accept_interface_message`] or
    /// [`Core::accept_interface_message_answerer`]. If `needs_answer` is false, you must call
    /// [`Core::accept_interface_message`].
    ///
    /// If `immediate` is true, you can additionally call
    /// [`Core::reject_immediate_interface_message`].
    InterfaceMessage {
        /// Id of the program that has emitted the message.
        pid: Pid,
        /// Identifier of the message that has been emitted.
        ///
        /// > **Note**: A [`MessageId`] is always generated for API-related purposes, even when no
        /// >           answer is expected.
        message_id: MessageId,
        /// True if the message is expecting an answer.
        needs_answer: bool,
        immediate: bool,
        /// Which interface the message has been emitted on.
        interface: InterfaceHash,
    },

    /// A process answered a message sent using [`Core::allocate_message_answerer`].
    AnsweredMessage {
        /// Answered message.
        message_id: MessageId,
        /// The answer in question.
        answer: Result<EncodedMessage, ()>,
    },
}

/// Additional information about a process.
#[derive(Debug)]
struct Process {
    /// Notifications available for retrieval by the process by calling `next_notification`.
    notifications_queue: notifications_queue::NotificationsQueue,

    /// List of threads that are frozen waiting for new notifications.
    wait_notifications_threads: waiting_threads::WaitingThreads,

    /// List of messages that the process is expected to answer.
    // TODO: do this in a smarter way
    messages_to_answer: Spinlock<SmallVec<[MessageId; 8]>>,
}

/// Access to a process within the core.
pub struct CoreProcess<'a, TExt: Extrinsics> {
    /// Access to the process within the inner collection.
    process: extrinsics::ProcAccess<'a, Process, (), TExt>,
}

impl<TExt: Extrinsics> Core<TExt> {
    /// Run the core once.
    pub async fn run(&self) -> CoreRunOutcome {
        loop {
            if let Some(ev) = self.run_inner().await {
                break ev;
            }
        }
    }

    /// Same as [`Core::run`]. Returns `None` if no event should be returned and we should loop
    /// again.
    async fn run_inner(&self) -> Option<CoreRunOutcome> {
        if let Some(ev) = self.pending_events.pop() {
            return Some(ev);
        }

        // Note: we use a temporary `run_outcome` variable in order to solve weird borrowing
        // issues. Feel free to try to remove it if you manage.
        let run_outcome = self.processes.run().await;
        match run_outcome {
            extrinsics::RunOneOutcome::ProcessFinished {
                pid,
                outcome,
                dead_threads: _,
                user_data,
            } => {
                Some(CoreRunOutcome::ProgramFinished {
                    pid,
                    unanswered_messages: Vec::new(), // TODO:
                    outcome,
                })
            }

            extrinsics::RunOneOutcome::ThreadFinished { .. } => {
                // TODO: report
                None
            }

            extrinsics::RunOneOutcome::ThreadWaitNotification(thread) => {
                // We immediately try to resume the thread with a notification.
                if let Err(thread) = try_resume_notification_wait_thread(thread) {
                    // If the thread couldn't be resumed, we add it to a list for later.
                    let tid = thread.tid();
                    thread
                        .process_user_data()
                        .wait_notifications_threads
                        .push(tid);
                }

                None
            }

            extrinsics::RunOneOutcome::ThreadEmitMessage(mut thread) => {
                let emitter_pid = thread.pid();
                let interface = thread.emit_interface().clone();
                let needs_answer = thread.needs_answer();
                let message_id = self.id_pool.assign();

                self.pending_accept_messages
                    .lock()
                    .insert(message_id, (emitter_pid, thread.tid()));

                Some(CoreRunOutcome::InterfaceMessage {
                    pid: emitter_pid,
                    message_id,
                    needs_answer,
                    immediate: !thread.allow_delay(),
                    interface,
                })
            }

            extrinsics::RunOneOutcome::ThreadEmitAnswer {
                message_id,
                process,
                ref response,
                ..
            } => {
                {
                    let mut messages_to_answer = process.user_data().messages_to_answer.lock();
                    if let Some(pos) = messages_to_answer.iter().position(|m| *m == message_id) {
                        messages_to_answer.remove(pos);
                    } else {
                        // TODO: crash the program? in any way, shouldn't panic
                        panic!()
                    }
                }

                let response = response.clone(); // TODO: why clone?
                self.answer_message_inner(message_id, Ok(response));
                None
            }

            extrinsics::RunOneOutcome::ThreadEmitMessageError {
                message_id,
                process,
                ..
            } => {
                {
                    let mut messages_to_answer = process.user_data().messages_to_answer.lock();
                    if let Some(pos) = messages_to_answer.iter().position(|m| *m == message_id) {
                        messages_to_answer.remove(pos);
                    } else {
                        // TODO: crash the program? in any way, shouldn't panic
                        panic!()
                    }
                }

                self.answer_message_inner(message_id, Err(()));
                None
            }

            extrinsics::RunOneOutcome::ThreadCancelMessage {
                message_id,
                process,
                ..
            } => {
                let mut pending_answer_messages = self.pending_answer_messages.lock();
                if let Entry::Occupied(entry) = pending_answer_messages.entry(message_id) {
                    if *entry.get() == process.pid() {
                        entry.remove();
                    }
                }

                None
            }
        }
    }

    /// Returns an object granting access to a process, if it exists.
    pub fn process_by_id(&self, pid: Pid) -> Option<CoreProcess<TExt>> {
        let p = self.processes.process_by_id(pid)?;
        Some(CoreProcess { process: p })
    }

    /// After [`CoreRunOutcome::InterfaceMessage`] is generated, use this method to accept the
    /// message. The message must later be answered with [`Core::answer_message`].
    pub fn accept_interface_message(&self, message_id: MessageId) -> EncodedMessage {
        // TODO: shouldn't unwrap if the process is already dead, but then what to return?

        let (pid, tid) = self
            .pending_accept_messages
            .lock()
            .remove(&message_id)
            .unwrap();
        match self.processes.interrupted_thread_by_id(tid).unwrap() {
            extrinsics::ThreadAccess::EmitMessage(thread) => {
                if thread.needs_answer() {
                    thread.accept_emit(Some(message_id))
                } else {
                    thread.accept_emit(None)
                }
            }
            _ => unreachable!(),
        }
    }

    /// After [`CoreRunOutcome::InterfaceMessage`] is generated, use this method to set the process
    /// that has the rights to answer this message.
    ///
    /// Unlocks the thread that was trying to emit the message, and returns the body of the
    /// message.
    ///
    /// > **Note**: The way the process in question is informed of the message is out of scope of
    /// >           this module.
    pub fn accept_interface_message_answerer(
        &self,
        message_id: MessageId,
        answerer_pid: Pid,
    ) -> EncodedMessage {
        // TODO: don't unwrap
        // TODO: is emitter_pid needed?
        let (emitter_pid, emitter_tid) = self
            .pending_accept_messages
            .lock()
            .remove(&message_id)
            .unwrap();

        let message = match self
            .processes
            .interrupted_thread_by_id(emitter_tid)
            .unwrap()
        {
            // TODO: don't unwrap
            extrinsics::ThreadAccess::EmitMessage(thread) => {
                if thread.needs_answer() {
                    thread.accept_emit(Some(message_id))
                } else {
                    thread.accept_emit(None)
                }
            }
            _ => unreachable!(),
        };

        self.pending_answer_messages
            .lock()
            .insert(message_id, emitter_pid);

        self.processes
            .process_by_id(answerer_pid)
            .unwrap() // TODO: immediately fail the message instead of unwrapping
            .user_data()
            .messages_to_answer
            .lock()
            .push(message_id);

        message
    }

    /// After [`CoreRunOutcome::InterfaceMessage`] is generated where `immediate` is true, use
    /// this method to notify that the message cannot be accepted at the moment.
    ///
    /// # Panic
    ///
    /// Panics if [`CoreRunOutcome::InterfaceMessage::immediate`] was false.
    /// Might panic if the message is in the wrong state.
    ///
    pub fn reject_immediate_interface_message(&self, message_id: MessageId) {
        let (pid, tid) = match self.pending_accept_messages.lock().remove(&message_id) {
            Some(v) => v,
            None => return,
        };

        match self.processes.interrupted_thread_by_id(tid) {
            Ok(extrinsics::ThreadAccess::EmitMessage(thread)) => {
                assert!(!thread.allow_delay());
                thread.refuse_emit();
            }
            Err(extrinsics::ThreadByIdErr::RunningOrDead) => {}
            _ => unreachable!(),
        }
    }

    /// Allocates a new message ID. The returned value is guaranteed to not be used for any further
    /// message.
    ///
    /// This [`MessageId`] isn't tracked by the [`Core`].
    pub fn allocate_untracked_message(&self) -> MessageId {
        self.id_pool.assign()
    }

    /// Allocates a new message ID. The given process is responsible for answering the message,
    /// similar to when [`Core::accept_interface_message_answerer`] is called.
    ///
    /// A [`CoreRunOutcome::AnsweredMessage`] will later be generated when the process answers
    /// this message.
    pub fn allocate_message_answerer(&self, answerer: Pid) -> MessageId {
        let message_id = self.id_pool.assign();
        self.processes
            .process_by_id(answerer)
            .unwrap() // TODO: immediately fail the message instead of unwrapping
            .user_data()
            .messages_to_answer
            .lock()
            .push(message_id);
        message_id
    }

    /// Set the answer to a message previously passed to [`Core::accept_interface_message`].
    // TODO: better API
    pub fn answer_message(&self, message_id: MessageId, response: Result<EncodedMessage, ()>) {
        self.answer_message_inner(message_id, response);
    }

    /// Common function for answering a message.
    fn answer_message_inner(&self, message_id: MessageId, response: Result<EncodedMessage, ()>) {
        let emitter_pid = match self.pending_answer_messages.lock().remove(&message_id) {
            Some(pid) => pid,
            None => return,
        };

        if let Some(process) = self.processes.process_by_id(emitter_pid) {
            process
                .user_data()
                .notifications_queue
                .push(message_id, response);
            self.try_resume_notification_wait(process);
        } else {
            // It is possible for the emitter of the message to have stopped or crashed, and we
            // had not updated `active_messages` yet.
        }
    }

    /// Start executing the module passed as parameter.
    ///
    /// Each import of the [`Module`](crate::module::Module) is resolved.
    pub fn execute(&self, module: &Module) -> Result<(CoreProcess<TExt>, ThreadId), vm::NewErr> {
        let proc_metadata = Process {
            notifications_queue: notifications_queue::NotificationsQueue::new(),
            messages_to_answer: Spinlock::new(SmallVec::new()),
            wait_notifications_threads: waiting_threads::WaitingThreads::new(),
        };

        let (process, main_tid) = self.processes.execute(module, proc_metadata, ())?;

        Ok((CoreProcess { process }, main_tid))
    }

    /// Tries to resume all the threads of the process that are waiting for an notification.
    fn try_resume_notification_wait(&self, process: extrinsics::ProcAccess<Process, (), TExt>) {
        // The actual work being done here is actually quite complicated in order to ensure that
        // each `ThreadId` is only accessed once at a time, but the exposed API is very simple.
        for thread_access in process.user_data().wait_notifications_threads.access() {
            let thread = match self
                .processes
                .interrupted_thread_by_id(thread_access.thread_id())
            {
                Ok(extrinsics::ThreadAccess::WaitNotification(thread)) => thread,
                _ => unreachable!(),
            };

            if try_resume_notification_wait_thread(thread).is_ok() {
                thread_access.remove();
            }
        }
    }
}

impl<'a, TExt: Extrinsics> CoreProcess<'a, TExt> {
    /// Returns the [`Pid`] of the process.
    pub fn pid(&self) -> Pid {
        self.process.pid()
    }

    /// Adds a new thread to the process, starting the function with the given index and passing
    /// the given parameters.
    pub fn start_thread(
        self,
        fn_index: u32,
        params: Vec<crate::WasmValue>,
    ) -> Result<(), vm::StartErr> {
        self.process.start_thread(fn_index, params, ())?;
        Ok(())
    }

    /// Starts killing the process.
    // TODO: more docs
    pub fn abort(&self) {
        self.process.abort();
    }
}

impl<TExt: Extrinsics> CoreBuilder<TExt> {
    /// Initializes a new [`CoreBuilder`].
    pub fn new() -> CoreBuilder<TExt> {
        CoreBuilder {
            inner_builder: extrinsics::Builder::default(),
        }
    }

    /// Allocates a `Pid` that will not be used by any process.
    ///
    /// > **Note**: As of the writing of this comment, this feature is only ever used to allocate
    /// >           `Pid`s that last forever. There is therefore no corresponding "unreserve_pid"
    /// >           method that frees such an allocated `Pid`. If there is ever a need to free
    /// >           these `Pid`s, such a method should be added.
    pub fn reserve_pid(&mut self) -> Pid {
        self.inner_builder.reserve_pid()
    }

    /// Turns the builder into a [`Core`].
    pub fn build(mut self) -> Core<TExt> {
        Core {
            pending_events: SegQueue::new(),
            processes: self.inner_builder.build(),
            id_pool: IdPool::new(),
            pending_accept_messages: Spinlock::new(HashMap::default()),
            pending_answer_messages: Spinlock::new(HashMap::default()),
        }
    }
}

/// If the given thread is waiting for a notification to arrive, checks the queue and tries to
/// resume said thread.
///
/// Returns back the thread within an `Err` if it couldn't be resumed.
fn try_resume_notification_wait_thread<TExt: Extrinsics>(
    mut thread: extrinsics::ThreadWaitNotif<Process, (), TExt>,
) -> Result<(), extrinsics::ThreadWaitNotif<Process, (), TExt>> {
    // Note that the code below is a bit weird and unelegant, but this is to bypass spurious
    // borrowing errors.
    let (entry_size, index_and_notif) = {
        // Try to find a notification in the queue that matches something the user is waiting for.
        // TODO: don't alloc a Vec
        let messages = thread.wait_entries().collect::<Vec<_>>();

        let entry = thread
            .process_user_data()
            .notifications_queue
            .find(&messages);

        let entry = match entry {
            Some(e) => e,
            None => {
                // No notification found.
                drop(entry);
                if !thread.block() {
                    thread.resume_no_notification();
                    return Ok(());
                } else {
                    return Err(thread);
                }
            }
        };

        let entry_size = entry.size();
        let index_and_notif = if entry_size <= thread.allowed_notification_size() {
            // Pop the notification from the queue for delivery.
            let index_in_msg_ids = entry.index_in_msg_ids();
            let notification = entry.extract();
            Some((index_in_msg_ids, notification))
        } else {
            None
        };

        (entry_size, index_and_notif)
    };

    if let Some((index_in_msg_ids, notification)) = index_and_notif {
        thread.resume_notification(index_in_msg_ids, notification)
    } else {
        thread.resume_notification_too_big(entry_size)
    }

    Ok(())
}
