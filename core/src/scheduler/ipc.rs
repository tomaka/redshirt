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

use crate::extrinsics::Extrinsics;
use crate::module::Module;
use crate::scheduler::{
    extrinsics::{self, ThreadAccessAccess as _},
    vm,
};
use crate::InterfaceHash;

use alloc::vec::Vec;
use crossbeam_queue::SegQueue;
use fnv::FnvBuildHasher;
use hashbrown::HashSet;
use nohash_hasher::BuildNoHashHasher;
use redshirt_syscalls::{Encode, EncodedMessage, MessageId, Pid, ThreadId};
use smallvec::SmallVec;
use spinning_top::Spinlock;

mod active_messages;
mod notifications_queue;
mod waiting_threads;

/// Handles scheduling processes and inter-process communications.
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
    /// Queue of events to return in priority when `run` is called.
    pending_events: SegQueue<CoreRunOutcome>,

    /// List of running processes.
    processes: extrinsics::ProcessesCollectionExtrinsics<Process, (), TExt>,

    /// List of [`Pid`]s that have been reserved during the construction.
    ///
    /// Never modified after initialization.
    reserved_pids: HashSet<Pid, BuildNoHashHasher<u64>>,

    /// List of messages that are waiting for an answer. Associates messages to their senders.
    active_messages: active_messages::ActiveMessages,
}

/// Prototype for a `Core` under construction.
pub struct CoreBuilder<TExt: Extrinsics> {
    /// See the corresponding field in [`Core`].
    reserved_pids: HashSet<Pid, BuildNoHashHasher<u64>>,
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
        /// List of messages sent using [`Core::allocate_message_answerer`] that the process was
        /// responsible for answering.
        ///
        /// One should treat the messages in this list as if a [`CoreRunOutcome::AnsweredMessage`]
        /// with `answer` equal to `Err(())` had been emitted for each of them.
        unanswered_messages: Vec<MessageId>,
        /// How the program ended. If `Ok`, it has gracefully terminated. If `Err`, something
        /// bad happened.
        // TODO: force Ok to i32?
        outcome: Result<Option<crate::WasmValue>, wasmi::Trap>,
    },

    /// A process has emitted a message on an interface.
    ///
    /// If the `message_id` is `Some`, you must call either [`CoreRunOutcome::set_answerer`] or
    /// [`CoreRunOutcome::answer_message`].
    InterfaceMessage {
        /// Id of the program that has emitted the message.
        pid: Pid,
        /// Identifier of the message that has been emitted. `None` if no answer is expected.
        message_id: Option<MessageId>,
        /// Which interface the message has been emitted on.
        interface: InterfaceHash,
        /// Message itself, as opaque bytes.
        message: EncodedMessage,
    },

    /// A process answered a message sent using [`Core::allocate_message_answerer`].
    AnsweredMessage {
        /// Answered message.
        message_id: MessageId,
        /// The answer in question.
        answer: Result<EncodedMessage, ()>,
    }
}

/// Additional information about a process.
#[derive(Debug)]
struct Process {
    /// Notifications available for retrieval by the process by calling `next_notification`.
    notifications_queue: notifications_queue::NotificationsQueue,

    /// Interfaces that the process has registered.
    registered_interfaces: Spinlock<SmallVec<[InterfaceHash; 1]>>,

    /// List of threads that are frozen waiting for new notifications.
    wait_notifications_threads: waiting_threads::WaitingThreads,

    /// List of interfaces that this process has used. When the process dies, we notify all the
    /// handlers about it.
    used_interfaces: HashSet<InterfaceHash, FnvBuildHasher>,

    /// List of messages that the process is expected to answer.
    messages_to_answer: SmallVec<[MessageId; 8]>,
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
        if let Ok(ev) = self.pending_events.pop() {
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
                // TODO: send message errors for interface messages that the process has received
                //       but not answered
                // TODO: empty the content of active_messages?

                // Notify interface handlers about the process stopping.
                //
                // Note that it is possible for the interface handler to be replaced by a
                // different one in a racy way, but that will result in a spurious process
                // destroyed notification, which isn't a problem.
                for interface in user_data.used_interfaces {
                    match self.interfaces.get(&interface) {
                        interface_handlers::Interface::Registered(handler_pid) => {
                            if let Some(process) = self.processes.process_by_id(handler_pid) {
                                process
                                    .user_data()
                                    .notifications_queue
                                    .push_process_destroyed_notification(pid);
                                self.try_resume_notification_wait(process);
                            } else {
                                // There's no need to emit a notification towards handlers with
                                // reserved PIDs, as the `ProgramFinished` event we are going to
                                // emit does the job.
                            }
                        }
                        // This can be reached if the interface handler has terminated
                        // in-between.
                        interface_handlers::Interface::Unregistered(_) => {}
                    }
                }

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
                // TODO: restore; plus we have to do the same for external messages
                /*thread
                .process_user_data()
                .used_interfaces
                .insert(interface.clone());*/

                match (self.interfaces.get(&interface), thread.allow_delay()) {
                    (interface_handlers::Interface::Registered(handler_pid), _) => {
                        let message_id = if thread.needs_answer() {
                            Some(self.active_messages.add_message(thread.pid()))
                        } else {
                            None
                        };

                        let message = thread.accept_emit(message_id);
                        if let Some(process) = self.processes.process_by_id(handler_pid) {
                            process
                                .user_data()
                                .notifications_queue
                                .push_interface_notification(
                                    &interface,
                                    message_id,
                                    emitter_pid,
                                    message,
                                );
                            self.try_resume_notification_wait(process);
                            None
                        } else if self.reserved_pids.contains(&handler_pid) {
                            Some(CoreRunOutcome::ReservedPidInterfaceMessage {
                                pid: emitter_pid,
                                message_id,
                                interface: interface.clone(),
                                message,
                            })
                        } else {
                            // This can be reached if a process has been killed but the list of
                            // interface handlers hasn't been updated yet.
                            // TODO: this is wrong; don't just ignore the message
                            None
                        }
                    }
                    (interface_handlers::Interface::Unregistered(..), false) => {
                        thread.refuse_emit();
                        None
                    }
                    (interface_handlers::Interface::Unregistered(reg), true) => {
                        reg.insert_waiting_thread(thread.tid());
                        Some(CoreRunOutcome::ThreadWaitUnavailableInterface {
                            thread_id: thread.tid(),
                            interface: interface.clone(),
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
                let response = response.clone(); // TODO: why clone?
                drop(run_outcome);
                self.answer_message_inner(message_id, Ok(response));
                None
            }

            extrinsics::RunOneOutcome::ThreadEmitMessageError { message_id, .. } => {
                // TODO: check ownership of the message
                drop(run_outcome);
                self.answer_message_inner(message_id, Err(()));
                None
            }

            extrinsics::RunOneOutcome::ThreadCancelMessage {
                message_id,
                process,
                ..
            } => {
                // Cancelling a message is implemented by simply removing it from the list of
                // active messages. For the sake of simplicity, no effort is for example being
                // made to maybe remove the notification destined to the interface handler.
                self.active_messages
                    .remove_if_emitted_by(message_id, process.pid());
                None
            }
        }
    }

    /// Returns an object granting access to a process, if it exists.
    pub fn process_by_id(&self, pid: Pid) -> Option<CoreProcess<TExt>> {
        let p = self.processes.process_by_id(pid)?;
        Some(CoreProcess { process: p })
    }

    /// Allocates a new message ID. The returned value is guaranteed to not be used for any further
    /// message.
    ///
    /// This [`MessageId`] isn't tracked by the [`Core`].
    pub fn allocate_untracked_message(&self) -> MessageId {
        todo!()
    }

    /// Allocates a new message ID. The given process is responsible for answering the message,
    /// similar to when [`Core::set_answerer`] is called.
    ///
    /// A [`CoreRunOutcome::AnsweredMessage`] will later be generated when the process answers
    /// this message.
    pub fn allocate_message_answerer(&self, answerer: Pid) -> MessageId {
        todo!()
    }

    /// After [`CoreRunOutcome::InterfaceMessage`] is generated, use this method to set the process
    /// that has the rights to answer this message.
    ///
    /// > **Note**: The way the process in question is informed of the message is out of scope of
    /// >           this module.
    pub fn set_answerer(&self, message_id: MessageId, pid: Pid) {
        todo!()
    }

    /// After [`CoreRunOutcome::InterfaceMessage`] is generated, use this method to send back an
    /// answer to that message.
    // TODO: better API
    pub fn answer_message(&self, message_id: MessageId, response: Result<EncodedMessage, ()>) {
        self.answer_message_inner(message_id, response);
    }

    /// Common function for answering a message.
    fn answer_message_inner(&self, message_id: MessageId, response: Result<EncodedMessage, ()>) {
        let emitter_pid = match self.active_messages.remove(message_id) {
            Some(pid) => pid,
            None => return,
        };

        if let Some(process) = self.processes.process_by_id(emitter_pid) {
            process
                .user_data()
                .notifications_queue
                .push(message_id, response);
            self.try_resume_notification_wait(process);
        } else if self.reserved_pids.contains(&emitter_pid) {
            self.pending_events.push(CoreRunOutcome::MessageResponse {
                message_id,
                response,
            });
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
            registered_interfaces: Spinlock::new(SmallVec::new()),
            used_interfaces: HashSet::with_hasher(Default::default()),
            messages_to_answer: SmallVec::new(),
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
            reserved_pids: HashSet::with_hasher(Default::default()),
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
        let pid = self.inner_builder.reserve_pid();
        let _was_inserted = self.reserved_pids.insert(pid);
        debug_assert!(_was_inserted);
        pid
    }

    /// Turns the builder into a [`Core`].
    pub fn build(mut self) -> Core<TExt> {
        self.reserved_pids.shrink_to_fit();

        Core {
            pending_events: SegQueue::new(),
            processes: self.inner_builder.build(),
            reserved_pids: self.reserved_pids,
            active_messages: active_messages::ActiveMessages::new(),
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
