// Copyright (C) 2019-2020-2020  Pierre Krieger
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

use crate::extrinsics::{Extrinsics, ExtrinsicsAction};
use crate::module::Module;
use crate::scheduler::{processes, vm};
use crate::sig;
use crate::{InterfaceHash, MessageId};

use alloc::{rc::Rc, vec, vec::Vec};
use byteorder::{ByteOrder as _, LittleEndian};
use core::{cell::RefCell, convert::TryFrom as _, fmt, mem};
use redshirt_syscalls::{EncodedMessage, Pid, ThreadId};

/// Wrapper around [`ProcessesCollection`](processes::ProcessesCollection), but that interprets
/// the extrinsic calls and keeps track of the state in which pending threads are in.
///
/// The generic parameters `TPud` and `TTud` are "user data"s that are stored respectively per
/// process and per thread, and allows the user to put extra information associated to a process
/// or a thread.
pub struct ProcessesCollectionExtrinsics<TPud, TTud, TExt: Extrinsics> {
    inner: RefCell<
        processes::ProcessesCollection<
            Extrinsic<TExt::ExtrinsicId>,
            LocalProcessUserData<TPud, TExt>,
            LocalThreadUserData<TTud, TExt::Context>,
        >,
    >,
    // TODO: implement
    /*/// List of processes that have died but that we haven't reported yet to the outside because
    /// they are locked.
    dead_processes: ,*/
}

/// Prototype for a `ProcessesCollectionExtrinsics` under construction.
pub struct ProcessesCollectionExtrinsicsBuilder<TExt: Extrinsics> {
    inner: processes::ProcessesCollectionBuilder<Extrinsic<TExt::ExtrinsicId>>,
}

/// Access to a process within the collection.
pub struct ProcessesCollectionExtrinsicsProc<'a, TPud, TTud, TExt: Extrinsics> {
    parent: &'a ProcessesCollectionExtrinsics<TPud, TTud, TExt>,
    pid: Pid,
    user_data: Rc<TPud>,
}

/// Access to a thread within the collection that is in an interrupted state.
///
/// Implements the [`ProcessesCollectionExtrinsicsThreadAccess`] trait.
pub enum ProcessesCollectionExtrinsicsThread<'a, TPud, TTud, TExt: Extrinsics> {
    EmitMessage(ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud, TExt>),
    WaitNotification(ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud, TExt>),
}

/// Access to a thread within the collection.
///
/// Implements the [`ProcessesCollectionExtrinsicsThreadAccess`] trait.
pub struct ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud, TExt: Extrinsics> {
    parent: &'a ProcessesCollectionExtrinsics<TPud, TTud, TExt>,
    tid: ThreadId,
    process_user_data: Rc<TPud>,

    /// External user data of the thread, extracted from the collection while the lock is held.
    ///
    /// Always `Some` while this struct is alive. Extracted only in the `Drop` implementation.
    thread_user_data: Option<TTud>,
}

/// Access to a thread within the collection.
///
/// Implements the [`ProcessesCollectionExtrinsicsThreadAccess`] trait.
pub struct ProcessesCollectionExtrinsicsThreadWaitNotification<
    'a,
    TPud,
    TTud,
    TExt: Extrinsics,
> {
    parent: &'a ProcessesCollectionExtrinsics<TPud, TTud, TExt>,
    tid: ThreadId,
    process_user_data: Rc<TPud>,

    /// External user data of the thread, extracted from the collection while the lock is held.
    ///
    /// Always `Some` while this struct is alive. Extracted only in the `Drop` implementation.
    thread_user_data: Option<TTud>,
}

/// Common trait amongst all the thread accessor structs.
pub trait ProcessesCollectionExtrinsicsThreadAccess<'a> {
    type ProcessUserData;
    type ThreadUserData;

    // TODO: make it return handle to process instead?

    /// Returns the id of the thread. Allows later retrieval by calling
    /// [`thread_by_id`](ProcessesCollectionExtrinsics::thread_by_id).
    ///
    /// [`ThreadId`]s are unique within a [`ProcessesCollectionExtrinsics`], independently from the
    /// process.
    fn tid(&mut self) -> ThreadId;

    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollectionExtrinsics::process_by_id).
    fn pid(&self) -> Pid;

    /// Returns the user data that is associated to the process.
    fn process_user_data(&self) -> &Self::ProcessUserData;

    /// Returns the user data that is associated to the thread.
    fn user_data(&mut self) -> &mut Self::ThreadUserData;
}

/// Error that can happen when calling `interrupted_thread_by_id`.
#[derive(Debug)]
pub enum ThreadByIdErr {
    /// Thread is either running, waiting to be run, dead, or has never existed.
    RunningOrDead,
    /// Thread is already locked.
    AlreadyLocked,
}

/// Possible function available to processes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Extrinsic<TExtId> {
    NextMessage,
    EmitMessage,
    EmitMessageError,
    EmitAnswer,
    CancelMessage,
    Other(TExtId),
}

/// Structure passed to the underlying [`processes::ProcessesCollection`] that tracks the state
/// of a process.
#[derive(Debug)]
struct LocalProcessUserData<TPud, TExt> {
    /// User data decided by the user.
    external_user_data: Rc<TPud>,
    /// Extrinsics supported by the process.
    extrinsics: TExt,
}

/// Structure passed to the underlying [`processes::ProcessesCollection`] that tracks the state
/// of a thread.
#[derive(Debug)]
struct LocalThreadUserData<TTud, TExtCtxt> {
    /// State of a thread.
    state: LocalThreadState<TExtCtxt>,
    /// User data decided by the user. When the thread is locked, this user data is extracted
    /// and stored locally in the lock. The data is put back when the thread is unlocked.
    external_user_data: Option<TTud>,
}

/// State of a thread. Private. Stored within the [`processes::ProcessesCollection`].
#[derive(Debug)]
enum LocalThreadState<TExtCtxt> {
    /// Thread is ready to run, running, or has just called an extrinsic and the call is being
    /// processed.
    ReadyToRun,

    /// Thread is running a non-hardcoded extrinsic.
    OtherExtrinsic(TExtCtxt, EmitMessage),

    /// The thread is sleeping and waiting for a notification to come.
    NotificationWait(NotificationWait),

    /// The thread called `emit_message` and wants to emit a message on an interface.
    EmitMessage(EmitMessage),
}

/// How a process is waiting for messages.
#[derive(Debug, PartialEq, Eq)]
struct NotificationWait {
    /// Identifiers of the notifications we are waiting upon. Copy of what is in the process's
    /// memory.
    notifs_ids: Vec<MessageId>,
    /// Offset within the memory of the process where the list of notifications to wait upon is
    /// located. This is necessary as we have to zero.
    notifs_ids_ptr: u32,
    /// Offset within the memory of the process where to write the received notifications.
    out_pointer: u32,
    /// Size of the memory of the process dedicated to receiving the notifications.
    out_size: u32,
    /// Whether to block the thread if no notification is available.
    block: bool,
}

/// How a process is emitting a message.
#[derive(Debug, PartialEq, Eq)]
struct EmitMessage {
    /// Interface we want to emit the message on.
    interface: InterfaceHash,
    /// Where to write back the message ID, or `None` if no answer is expected.
    message_id_write: Option<u32>,
    /// Message itself. Needs to be delivered to the handler once it is registered.
    message: EncodedMessage,
    /// True if we're allowed to block the thread to wait for an interface handler to be
    /// available.
    allow_delay: bool,
}

/// How a process is emitting a response.
#[derive(Debug, PartialEq, Eq)]
struct EmitAnswer {
    /// Message to answer.
    message_id: MessageId,
    /// The response itself.
    response: EncodedMessage,
}

/// Outcome of the [`run`](ProcessesCollectionExtrinsics::run) function.
#[derive(Debug)]
pub enum RunOneOutcome<'a, TPud, TTud, TExt: Extrinsics> {
    /// Either the main thread of a process has finished, or a fatal error was encountered.
    ///
    /// The process no longer exists.
    ProcessFinished {
        /// Pid of the process that has finished.
        pid: Pid,

        /// User data of the process.
        user_data: TPud,

        /// Id and user datas of all the threads of the process. The first element is the main
        /// thread's.
        /// These threads no longer exist.
        dead_threads: Vec<(ThreadId, TTud)>,

        /// Value returned by the main thread that has finished, or error that happened.
        outcome: Result<Option<wasmi::RuntimeValue>, wasmi::Trap>,
    },

    /// A thread in a process has finished.
    ThreadFinished {
        /// Thread which has finished.
        thread_id: ThreadId,

        /// Process whose thread has finished.
        process: ProcessesCollectionExtrinsicsProc<'a, TPud, TTud, TExt>,

        /// User data of the thread.
        user_data: TTud,

        /// Value returned by the function that was executed.
        value: Option<wasmi::RuntimeValue>,
    },

    /// A thread in a process wants to emit a message.
    ThreadEmitMessage(ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud, TExt>),

    /// A thread in a process is waiting for an incoming message.
    ThreadWaitNotification(
        ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud, TExt>,
    ),

    /// A thread in a process wants to answer a message.
    ThreadEmitAnswer {
        /// Thread that wants to emit an answer.
        thread_id: ThreadId,

        /// Process that the thread belongs to.
        process: ProcessesCollectionExtrinsicsProc<'a, TPud, TTud, TExt>,

        /// Message to answer.
        message_id: MessageId,

        /// The answer it self.
        response: EncodedMessage,
    },

    /// A thread in a process wants to notify that a message is erroneous.
    ThreadEmitMessageError {
        /// Thread that wants to emit a message error.
        thread_id: ThreadId,

        /// Process that the thread belongs to.
        process: ProcessesCollectionExtrinsicsProc<'a, TPud, TTud, TExt>,

        /// Message that is erroneous.
        message_id: MessageId,
    },

    /// A thread in a process wants to notify that a message is to be cancelled.
    ThreadCancelMessage {
        /// Thread that wants to emit a cancellation.
        thread_id: ThreadId,

        /// Process that the thread belongs to.
        process: ProcessesCollectionExtrinsicsProc<'a, TPud, TTud, TExt>,

        /// Message that must be cancelled.
        message_id: MessageId,
    },

    /// No thread is ready to run. Nothing was done.
    Idle,
}

impl<TPud, TTud, TExt> ProcessesCollectionExtrinsics<TPud, TTud, TExt>
where
    TExt: Extrinsics,
{
    /// Creates a new process state machine from the given module.
    ///
    /// The closure is called for each import that the module has. It must assign a number to each
    /// import, or return an error if the import can't be resolved. When the VM calls one of these
    /// functions, this number will be returned back in order for the user to know how to handle
    /// the call.
    ///
    /// A single main thread (whose user data is passed by parameter) is automatically created and
    /// is paused at the start of the "_start" function of the module.
    pub fn execute(
        &self,
        module: &Module,
        proc_user_data: TPud,
        main_thread_user_data: TTud,
    ) -> Result<ProcessesCollectionExtrinsicsProc<TPud, TTud, TExt>, vm::NewErr> {
        let external_user_data = Rc::new(proc_user_data);
        let proc_user_data = LocalProcessUserData {
            external_user_data: external_user_data.clone(),
            extrinsics: Default::default(),
        };
        let main_thread_user_data = LocalThreadUserData {
            state: LocalThreadState::ReadyToRun,
            external_user_data: Some(main_thread_user_data),
        };
        let pid = self
            .inner
            .borrow_mut()
            .execute(module, proc_user_data, main_thread_user_data)?
            .pid();
        Ok(ProcessesCollectionExtrinsicsProc {
            parent: self,
            pid,
            user_data: external_user_data,
        })
    }

    /// Runs one thread amongst the collection.
    ///
    /// Which thread is run is implementation-defined and no guarantee is made.
    pub fn run(&self) -> RunOneOutcome<TPud, TTud, TExt> {
        let mut inner = self.inner.borrow_mut();
        match inner.run() {
            processes::RunOneOutcome::ProcessFinished {
                pid,
                user_data,
                dead_threads,
                outcome,
            } => {
                // If the process isn't locked, we immediately report that the process has
                // finished.
                if Rc::strong_count(&user_data.external_user_data) == 1 {
                    return RunOneOutcome::ProcessFinished {
                        pid,
                        user_data: match Rc::try_unwrap(user_data.external_user_data) {
                            Ok(ud) => ud,
                            Err(_) => panic!(),
                        },
                        dead_threads: dead_threads
                            .into_iter()
                            .map(|(id, state)| (id, state.external_user_data.unwrap()))
                            .collect(), // TODO: meh for allocation
                        outcome,
                    };
                }

                // TODO: hold a list of dead processes; not needed at the moment because we are
                // single-threaded and the caller doesn't hold proc locks for a long time
                unimplemented!()
            }
            processes::RunOneOutcome::ThreadFinished {
                process,
                user_data,
                value,
                thread_id,
            } => {
                debug_assert!(user_data.state.is_ready_to_run());
                RunOneOutcome::ThreadFinished {
                    thread_id,
                    process: self.process_by_id(process.pid()).unwrap(),
                    user_data: match user_data.external_user_data {
                        Some(ud) => ud,
                        None => panic!(),
                    },
                    value,
                }
            }
            processes::RunOneOutcome::Idle => RunOneOutcome::Idle,

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::NextMessage,
                params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                let next_msg = match parse_extrinsic_next_notification(&mut thread, params) {
                    Ok(m) => m,
                    Err(_) => panic!(), // TODO:
                };
                thread.user_data().state = LocalThreadState::NotificationWait(next_msg);
                let process_user_data = thread.process_user_data().external_user_data.clone();
                let thread_user_data = thread.user_data().external_user_data.take().unwrap();
                RunOneOutcome::ThreadWaitNotification(
                    ProcessesCollectionExtrinsicsThreadWaitNotification {
                        parent: self,
                        tid: thread.tid(),
                        process_user_data,
                        thread_user_data: Some(thread_user_data),
                    },
                )
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::EmitMessage,
                params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                let emit_msg = match parse_extrinsic_emit_message(&mut thread, params) {
                    Ok(m) => m,
                    Err(_) => panic!(), // TODO:
                };
                thread.user_data().state = LocalThreadState::EmitMessage(emit_msg);
                let process_user_data = thread.process_user_data().external_user_data.clone();
                let thread_user_data = thread.user_data().external_user_data.take().unwrap();
                RunOneOutcome::ThreadEmitMessage(ProcessesCollectionExtrinsicsThreadEmitMessage {
                    parent: self,
                    tid: thread.tid(),
                    process_user_data,
                    thread_user_data: Some(thread_user_data),
                })
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::EmitAnswer,
                params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                debug_assert!(thread.user_data().external_user_data.is_some());
                let emit_resp = match parse_extrinsic_emit_answer(&mut thread, params) {
                    Ok(m) => m,
                    Err(_) => panic!(), // TODO:
                };
                thread.resume(None);
                let pid = thread.pid();
                let thread_id = thread.tid();
                let proc_user_data = inner
                    .process_by_id(pid)
                    .unwrap()
                    .user_data()
                    .external_user_data
                    .clone();
                RunOneOutcome::ThreadEmitAnswer {
                    process: ProcessesCollectionExtrinsicsProc {
                        parent: self,
                        pid,
                        user_data: proc_user_data,
                    },
                    thread_id,
                    message_id: emit_resp.message_id,
                    response: emit_resp.response,
                }
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::EmitMessageError,
                params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                debug_assert!(thread.user_data().external_user_data.is_some());
                let emit_msg_error = match parse_extrinsic_emit_message_error(&mut thread, params) {
                    Ok(m) => m,
                    Err(_) => panic!(), // TODO:
                };
                thread.resume(None);
                let pid = thread.pid();
                let thread_id = thread.tid();
                let proc_user_data = inner
                    .process_by_id(pid)
                    .unwrap()
                    .user_data()
                    .external_user_data
                    .clone();
                RunOneOutcome::ThreadEmitMessageError {
                    process: ProcessesCollectionExtrinsicsProc {
                        parent: self,
                        pid,
                        user_data: proc_user_data,
                    },
                    thread_id,
                    message_id: emit_msg_error,
                }
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::CancelMessage,
                params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                debug_assert!(thread.user_data().external_user_data.is_some());
                let emit_cancel = match parse_extrinsic_cancel_message(&mut thread, params) {
                    Ok(m) => m,
                    Err(_) => panic!(), // TODO:
                };
                thread.resume(None);
                let pid = thread.pid();
                let thread_id = thread.tid();
                let proc_user_data = inner
                    .process_by_id(pid)
                    .unwrap()
                    .user_data()
                    .external_user_data
                    .clone();
                RunOneOutcome::ThreadCancelMessage {
                    process: ProcessesCollectionExtrinsicsProc {
                        parent: self,
                        pid,
                        user_data: proc_user_data,
                    },
                    thread_id,
                    message_id: emit_cancel,
                }
            }

            processes::RunOneOutcome::Interrupted {
                ref mut thread,
                id: Extrinsic::Other(ext_id),
                ref params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                let thread_id = thread.tid();
                let (context, action) = thread
                    .process_user_data()
                    .extrinsics
                    .new_context(thread_id, ext_id, params);
                match action {
                    ExtrinsicsAction::ProgramCrash => unimplemented!(),
                    ExtrinsicsAction::Resume(value) => thread.resume(value),
                    ExtrinsicsAction::EmitMessage { interface, message, response_expected } => {
                        thread.user_data().state = LocalThreadState::OtherExtrinsic(context);
                        unimplemented!()
                    },
                }
            }
        }
    }

    /// Returns a process by its [`Pid`], if it exists.
    ///
    /// This function returns a "lock".
    /// While the lock is held, it isn't possible for a [`RunOneOutcome::ProcessFinished`]
    /// message to be returned.
    ///
    /// If a program crashes or finishes while a lock is held, it is marked as dying and the
    /// termination is delayed until the point when all locks have been released.
    pub fn process_by_id(
        &self,
        pid: Pid,
    ) -> Option<ProcessesCollectionExtrinsicsProc<TPud, TTud, TExt>> {
        let mut inner = self.inner.borrow_mut();
        let inner = inner.process_by_id(pid)?;
        Some(ProcessesCollectionExtrinsicsProc {
            parent: self,
            pid,
            user_data: inner.user_data().external_user_data.clone(),
        })
    }

    /// Returns a thread by its [`ThreadId`], if it exists and is not running.
    ///
    /// It is only possible to access threads that aren't currently running.
    ///
    /// This function returns a "lock".
    /// Calling `interrupted_thread_by_id` again on the same thread will return
    /// `Err(ThreadByIdErr::AlreadyLocked)`.
    ///
    /// This lock is also implicitely a lock against the process that owns the thread.
    /// See [`ProcessesCollectionExtrinsics::process_by_id`].
    pub fn interrupted_thread_by_id(
        &self,
        id: ThreadId,
    ) -> Result<ProcessesCollectionExtrinsicsThread<TPud, TTud, TExt>, ThreadByIdErr> {
        let mut inner = self.inner.borrow_mut();
        let mut inner = inner.thread_by_id(id).ok_or(ThreadByIdErr::RunningOrDead)?;

        // Checking thread locked state.
        if inner.user_data().external_user_data.is_none() {
            return Err(ThreadByIdErr::AlreadyLocked);
        }

        match inner.user_data().state {
            LocalThreadState::ReadyToRun => {
                debug_assert!(inner.user_data().external_user_data.is_some());
                Err(ThreadByIdErr::RunningOrDead)
            }
            LocalThreadState::EmitMessage(_) | LocalThreadState::OtherExtrinsic(_) => {
                let process_user_data = inner.process_user_data().external_user_data.clone();
                let thread_user_data = inner.user_data().external_user_data.take().unwrap();

                Ok(From::from(ProcessesCollectionExtrinsicsThreadEmitMessage {
                    parent: self,
                    tid: id,
                    process_user_data,
                    thread_user_data: Some(thread_user_data),
                }))
            }
            LocalThreadState::NotificationWait(_) => {
                let process_user_data = inner.process_user_data().external_user_data.clone();
                let thread_user_data = inner.user_data().external_user_data.take().unwrap();

                Ok(From::from(
                    ProcessesCollectionExtrinsicsThreadWaitNotification {
                        parent: self,
                        tid: id,
                        process_user_data,
                        thread_user_data: Some(thread_user_data),
                    },
                ))
            }
        }
    }
}

impl<TExt> Default for ProcessesCollectionExtrinsicsBuilder<TExt>
where
    TExt: Extrinsics
{
    fn default() -> Self {
        let mut inner = processes::ProcessesCollectionBuilder::default()
            .with_extrinsic(
                "redshirt",
                "next_notification",
                sig!((I32, I32, I32, I32, I32) -> I32),
                Extrinsic::NextMessage,
            )
            .with_extrinsic(
                "redshirt",
                "emit_message",
                sig!((I32, I32, I32, I32, I32, I32) -> I32),
                Extrinsic::EmitMessage,
            )
            .with_extrinsic(
                "redshirt",
                "emit_message_error",
                sig!((I32)),
                Extrinsic::EmitMessageError,
            )
            .with_extrinsic(
                "redshirt",
                "emit_answer",
                sig!((I32, I32, I32)),
                Extrinsic::EmitAnswer,
            )
            .with_extrinsic(
                "redshirt",
                "cancel_message",
                sig!((I32)),
                Extrinsic::CancelMessage,
            );

        for supported in TExt::supported_extrinsics() {
            inner = inner.with_extrinsic(
                supported.wasm_interface,
                supported.function_name,
                supported.signature,
                Extrinsic::Other(supported.id),
            );
        }

        ProcessesCollectionExtrinsicsBuilder {
            inner,
        }
    }
}

impl<TExt: Extrinsics> ProcessesCollectionExtrinsicsBuilder<TExt> {
    /// Allocates a `Pid` that will not be used by any process.
    ///
    /// > **Note**: As of the writing of this comment, this feature is only ever used to allocate
    /// >           `Pid`s that last forever. There is therefore no corresponding "unreserve_pid"
    /// >           method that frees such an allocated `Pid`. If there is ever a need to free
    /// >           these `Pid`s, such a method should be added.
    pub fn reserve_pid(&mut self) -> Pid {
        self.inner.reserve_pid()
    }

    /// Turns the builder into a [`ProcessesCollectionExtrinsics`].
    pub fn build<TPud, TTud>(self) -> ProcessesCollectionExtrinsics<TPud, TTud, TExt> {
        ProcessesCollectionExtrinsics {
            inner: RefCell::new(self.inner.build()),
        }
    }
}

impl<'a, TPud, TTud, TExt> ProcessesCollectionExtrinsicsProc<'a, TPud, TTud, TExt>
where
    TExt: Extrinsics,
{
    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollection::process_by_id).
    pub fn pid(&self) -> Pid {
        self.pid
    }

    /// Returns the user data that is associated to the process.
    pub fn user_data(&self) -> &TPud {
        &self.user_data
    }

    /// Adds a new thread to the process, starting the function with the given index and passing
    /// the given parameters.
    ///
    /// > **Note**: The "function ID" is the index of the function in the WASM module. WASM
    /// >           doesn't have function pointers. Instead, all the functions are part of a single
    /// >           global array of functions.
    // TODO: don't expose wasmi::RuntimeValue in the API
    pub fn start_thread(
        &self,
        fn_index: u32,
        params: Vec<wasmi::RuntimeValue>,
        user_data: TTud,
    ) -> Result<(), vm::StartErr> {
        let mut inner = self.parent.inner.borrow_mut();
        let inner = inner.process_by_id(self.pid).unwrap();

        inner.start_thread(
            fn_index,
            params,
            LocalThreadUserData {
                state: LocalThreadState::ReadyToRun,
                external_user_data: Some(user_data),
            },
        )?;

        Ok(())
    }

    /// Returns a list of all threads that are in an interrupted state.
    // TODO: what about the threads that are interrupted by already locked?
    // TODO: implement better
    pub fn interrupted_threads(
        &self,
    ) -> impl Iterator<Item = ProcessesCollectionExtrinsicsThread<'a, TPud, TTud, TExt>> {
        let mut inner = self.parent.inner.borrow_mut();
        let inner = inner.process_by_id(self.pid).unwrap();

        let mut out = Vec::new();

        let mut thread = Some(inner.main_thread());
        while let Some(mut thread_inner) = thread.take() {
            out.push(thread_inner.tid());
            thread = thread_inner.next_thread();
        }

        let parent = self.parent;
        out.into_iter().filter_map(move |tid| {
            match parent.interrupted_thread_by_id(tid) {
                Ok(t) => Some(t),
                Err(ThreadByIdErr::AlreadyLocked) => unimplemented!(), // TODO: what to do here?
                Err(ThreadByIdErr::RunningOrDead) => None,
            }
        })
    }

    /// Marks the process as aborting.
    ///
    /// The termination will happen after all locks to this process have been released.
    ///
    /// Calling [`abort`] a second time or more has no effect.
    pub fn abort(&self) {
        unimplemented!() // TODO:
    }
}

impl<'a, TPud, TTud, TExt> fmt::Debug for ProcessesCollectionExtrinsicsProc<'a, TPud, TTud, TExt>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
    TExt: Extrinsics,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: improve
        f.debug_tuple("ProcessesCollectionExtrinsicsProc").finish()
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics>
    From<ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud, TExt>>
    for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud, TExt>
{
    fn from(
        thread: ProcessesCollectionExtrinsicsThreadEmitMessage<
            'a,
            TPud,
            TTud,
            TExt,
        >,
    ) -> Self {
        ProcessesCollectionExtrinsicsThread::EmitMessage(thread)
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics>
    From<ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud, TExt>>
    for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud, TExt>
{
    fn from(
        thread: ProcessesCollectionExtrinsicsThreadWaitNotification<
            'a,
            TPud,
            TTud,
            TExt,
        >,
    ) -> Self {
        ProcessesCollectionExtrinsicsThread::WaitNotification(thread)
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> ProcessesCollectionExtrinsicsThreadAccess<'a>
    for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud, TExt>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&mut self) -> ThreadId {
        match self {
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => t.tid(),
            ProcessesCollectionExtrinsicsThread::WaitNotification(t) => t.tid(),
        }
    }

    fn pid(&self) -> Pid {
        match self {
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => t.pid(),
            ProcessesCollectionExtrinsicsThread::WaitNotification(t) => t.pid(),
        }
    }

    fn process_user_data(&self) -> &TPud {
        match self {
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => t.process_user_data(),
            ProcessesCollectionExtrinsicsThread::WaitNotification(t) => t.process_user_data(),
        }
    }

    fn user_data(&mut self) -> &mut TTud {
        match self {
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => t.user_data(),
            ProcessesCollectionExtrinsicsThread::WaitNotification(t) => t.user_data(),
        }
    }
}

impl<'a, TPud, TTud, TExt> fmt::Debug for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud, TExt>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
    TExt: Extrinsics,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => fmt::Debug::fmt(t, f),
            ProcessesCollectionExtrinsicsThread::WaitNotification(t) => fmt::Debug::fmt(t, f),
        }
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics>
    ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud, TExt>
{
    /// Returns true if the caller wants an answer to the message.
    pub fn needs_answer(&mut self) -> bool {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();

        if let LocalThreadState::EmitMessage(ref emit) = inner.user_data().state {
            emit.message_id_write.is_some()
        } else {
            unreachable!()
        }
    }

    /// Returns the interface to emit the message on.
    pub fn emit_interface(&mut self) -> InterfaceHash {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();

        if let LocalThreadState::EmitMessage(ref emit) = inner.user_data().state {
            // TODO: cloning :-/
            emit.interface.clone()
        } else {
            unreachable!()
        }
    }

    /// True if the caller allows delays.
    pub fn allow_delay(&mut self) -> bool {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();

        if let LocalThreadState::EmitMessage(ref emit) = inner.user_data().state {
            emit.allow_delay
        } else {
            unreachable!()
        }
    }

    /// Returns the message to emit and resumes the thread.
    ///
    /// # Panic
    ///
    /// - Panics if `message_id.is_some() != thread.needs_answer()`. In other words, if
    /// `needs_answer` is true, then you **must** provide a `MessageId`.
    ///
    pub fn accept_emit(self, message_id: Option<MessageId>) -> EncodedMessage {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();

        let emit = {
            match mem::replace(&mut inner.user_data().state, LocalThreadState::ReadyToRun) {
                LocalThreadState::EmitMessage(emit) => emit,
                _ => unreachable!(),
            }
        };

        if let Some(message_id_write) = emit.message_id_write {
            let message_id = match message_id {
                Some(m) => m,
                None => panic!(),
            };

            let mut buf = [0; 8];
            LittleEndian::write_u64(&mut buf, From::from(message_id));
            inner.write_memory(message_id_write, &buf).unwrap();
        } else {
            assert!(message_id.is_none());
        }

        inner.resume(Some(wasmi::RuntimeValue::I32(0)));
        emit.message
    }

    /// Resumes the thread, signalling an error in the emission.
    pub fn refuse_emit(self) {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();
        inner.resume(Some(wasmi::RuntimeValue::I32(1)));
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> ProcessesCollectionExtrinsicsThreadAccess<'a>
    for ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud, TExt>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&mut self) -> ThreadId {
        self.tid
    }

    fn pid(&self) -> Pid {
        let mut inner = self.parent.inner.borrow_mut();
        inner.thread_by_id(self.tid).unwrap().pid()
    }

    fn process_user_data(&self) -> &TPud {
        &self.process_user_data
    }

    fn user_data(&mut self) -> &mut TTud {
        self.thread_user_data.as_mut().unwrap()
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> Drop
    for ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud, TExt>
{
    fn drop(&mut self) {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();
        let external_user_data = &mut inner.user_data().external_user_data;
        debug_assert!(external_user_data.is_none());
        *external_user_data = Some(self.thread_user_data.take().unwrap());
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> fmt::Debug
    for ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud, TExt>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: improve
        f.debug_tuple("ProcessesCollectionExtrinsicsThreadEmitMessage")
            .finish()
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics>
    ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud, TExt>
{
    /// Returns the list of message IDs that the thread is waiting on. In order.
    // TODO: not great naming. we're waiting either for messages or an interface notif or a process cancelled notif
    pub fn message_ids_iter<'b>(&'b mut self) -> impl Iterator<Item = MessageId> + 'b {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();

        if let LocalThreadState::NotificationWait(ref wait) = inner.user_data().state {
            // TODO: annoying allocation
            wait.notifs_ids
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
        } else {
            unreachable!()
        }
    }

    /// Returns the maximum size allowed for a notification.
    pub fn allowed_notification_size(&mut self) -> usize {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();

        if let LocalThreadState::NotificationWait(ref wait) = inner.user_data().state {
            usize::try_from(wait.out_size).unwrap()
        } else {
            unreachable!()
        }
    }

    /// Returns true if we should block the thread waiting for a notification to come.
    pub fn block(&mut self) -> bool {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();

        if let LocalThreadState::NotificationWait(ref wait) = inner.user_data().state {
            wait.block
        } else {
            unreachable!()
        }
    }

    /// Resume the thread, sending back a notification.
    ///
    /// `index` must be the index within the list returned by [`message_ids_iter`].
    ///
    /// # Panic
    ///
    /// - Panics if the notification is too large. You should make sure this is not the case before
    /// calling this function.
    /// - Panics if `index` is too large.
    ///
    pub fn resume_notification(self, index: usize, notif: EncodedMessage) {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();

        let wait = {
            match mem::replace(&mut inner.user_data().state, LocalThreadState::ReadyToRun) {
                LocalThreadState::NotificationWait(wait) => wait,
                _ => unreachable!(),
            }
        };

        assert!(index < wait.notifs_ids.len());
        let notif_size_u32 = u32::try_from(notif.0.len()).unwrap();
        assert!(wait.out_size >= notif_size_u32);

        // Write the notification in the process's memory.
        match inner.write_memory(wait.out_pointer, &notif.0) {
            Ok(()) => {}
            Err(_) => panic!(), // TODO: can legit happen
        };

        // Zero the corresponding entry in the notifications to wait upon.
        match inner.write_memory(
            wait.notifs_ids_ptr + u32::try_from(index).unwrap() * 8,
            &[0; 8],
        ) {
            Ok(()) => {}
            Err(_) => panic!(), // TODO: can legit happen
        };

        inner.user_data().state = LocalThreadState::ReadyToRun;
        inner.resume(Some(wasmi::RuntimeValue::I32(
            i32::try_from(notif_size_u32).unwrap(),
        )));
    }

    /// Resume the thread, indicating that the notification is too large for the provided buffer.
    pub fn resume_notification_too_big(self, notif_size: usize) {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();

        debug_assert!({
            let expected = match &mut inner.user_data().state {
                LocalThreadState::NotificationWait(wait) => wait.out_size,
                _ => unreachable!(),
            };
            expected < u32::try_from(notif_size).unwrap()
        });

        inner.user_data().state = LocalThreadState::ReadyToRun;
        inner.resume(Some(wasmi::RuntimeValue::I32(
            i32::try_from(notif_size).unwrap(),
        )));
    }

    /// Resume the thread, indicating that no notification is available.
    ///
    /// # Panic
    ///
    /// - Panics if [`block`](ProcessesCollectionExtrinsicsThreadWaitNotification::block) would
    /// return `true`.
    ///
    pub fn resume_no_notification(self) {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();

        if let LocalThreadState::NotificationWait(ref wait) = inner.user_data().state {
            assert!(!wait.block);
        } else {
            unreachable!()
        }

        inner.user_data().state = LocalThreadState::ReadyToRun;
        inner.resume(Some(wasmi::RuntimeValue::I32(0)));
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> ProcessesCollectionExtrinsicsThreadAccess<'a>
    for ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud, TExt>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&mut self) -> ThreadId {
        self.tid
    }

    fn pid(&self) -> Pid {
        let mut inner = self.parent.inner.borrow_mut();
        inner.thread_by_id(self.tid).unwrap().pid()
    }

    fn process_user_data(&self) -> &TPud {
        &self.process_user_data
    }

    fn user_data(&mut self) -> &mut TTud {
        self.thread_user_data.as_mut().unwrap()
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> Drop
    for ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud, TExt>
{
    fn drop(&mut self) {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.thread_by_id(self.tid).unwrap();
        let external_user_data = &mut inner.user_data().external_user_data;
        debug_assert!(external_user_data.is_none());
        *external_user_data = Some(self.thread_user_data.take().unwrap());
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> fmt::Debug
    for ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud, TExt>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: improve
        f.debug_tuple("ProcessesCollectionExtrinsicsThreadWaitNotification")
            .finish()
    }
}

impl<TExtCtxt> LocalThreadState<TExtCtxt> {
    /// True if `self` is equal to [`LocalThreadState::ReadyToRun`].
    fn is_ready_to_run(&self) -> bool {
        match self {
            LocalThreadState::ReadyToRun => true,
            _ => false,
        }
    }
}

/// Analyzes a call to `next_notification` made by the given thread.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
fn parse_extrinsic_next_notification<TPud, TTud, TExtCtxt>(
    thread: &mut processes::ProcessesCollectionThread<TPud, LocalThreadUserData<TTud, TExtCtxt>>,
    params: Vec<wasmi::RuntimeValue>,
) -> Result<NotificationWait, ExtrinsicNextNotificationErr> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 5);

    let notifs_ids_ptr = u32::try_from(
        params[0]
            .try_into::<i32>()
            .ok_or(ExtrinsicNextNotificationErr::BadParameter)?,
    )
    .map_err(|_| ExtrinsicNextNotificationErr::BadParameter)?;
    // TODO: consider not copying the notification ids and read memory on demand instead
    let notifs_ids = {
        let len = u32::try_from(
            params[1]
                .try_into::<i32>()
                .ok_or(ExtrinsicNextNotificationErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicNextNotificationErr::BadParameter)?;
        if len >= 512 {
            // TODO: arbitrary limit in order to not allocate too much memory below; a bit crappy
            return Err(ExtrinsicNextNotificationErr::TooManyNotificationIds { requested: len });
        }
        let mem = thread
            .read_memory(notifs_ids_ptr, len * 8)
            .map_err(|_| ExtrinsicNextNotificationErr::BadParameter)?;
        let len_usize = usize::try_from(len)
            .map_err(|_| ExtrinsicNextNotificationErr::TooManyNotificationIds { requested: len })?;
        let mut out = vec![MessageId::from(0u64); len_usize];
        for (o, i) in out.iter_mut().zip(mem.chunks(8)) {
            let val = byteorder::LittleEndian::read_u64(i);
            *o = MessageId::from(val);
        }
        out
    };

    let out_pointer = u32::try_from(
        params[2]
            .try_into::<i32>()
            .ok_or(ExtrinsicNextNotificationErr::BadParameter)?,
    )
    .map_err(|_| ExtrinsicNextNotificationErr::BadParameter)?;
    let out_size = u32::try_from(
        params[3]
            .try_into::<i32>()
            .ok_or(ExtrinsicNextNotificationErr::BadParameter)?,
    )
    .map_err(|_| ExtrinsicNextNotificationErr::BadParameter)?;
    let block = params[4]
        .try_into::<i32>()
        .ok_or(ExtrinsicNextNotificationErr::BadParameter)?
        != 0;

    Ok(NotificationWait {
        notifs_ids,
        notifs_ids_ptr,
        out_pointer,
        out_size,
        block,
    })
}

/// Error that [`parse_extrinsic_next_notification`] can return.
#[derive(Debug)]
enum ExtrinsicNextNotificationErr {
    /// Too many notification ids requested.
    TooManyNotificationIds {
        /// Number of notification IDs that have been requested.
        requested: u32,
    },
    /// Bad type or invalid value for a parameter.
    BadParameter,
}

/// Analyzes a call to `emit_message` made by the given thread.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
fn parse_extrinsic_emit_message<TPud, TTud, TExtCtxt>(
    thread: &mut processes::ProcessesCollectionThread<TPud, LocalThreadUserData<TTud, TExtCtxt>>,
    params: Vec<wasmi::RuntimeValue>,
) -> Result<EmitMessage, ExtrinsicEmitMessageErr> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 6);

    let interface: InterfaceHash = {
        let addr = u32::try_from(
            params[0]
                .try_into::<i32>()
                .ok_or(ExtrinsicEmitMessageErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?;
        InterfaceHash::from(
            <[u8; 32]>::try_from(
                &thread
                    .read_memory(addr, 32)
                    .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?[..],
            )
            .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?,
        )
    };

    let message = {
        let addr = u32::try_from(
            params[1]
                .try_into::<i32>()
                .ok_or(ExtrinsicEmitMessageErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?;
        let num_bufs = u32::try_from(
            params[2]
                .try_into::<i32>()
                .ok_or(ExtrinsicEmitMessageErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?;
        let mut out_msg = Vec::new();
        for buf_n in 0..num_bufs {
            let sub_buf_ptr = thread
                .read_memory(addr + 8 * buf_n, 4)
                .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?;
            let sub_buf_ptr = LittleEndian::read_u32(&sub_buf_ptr);
            let sub_buf_sz = thread
                .read_memory(addr + 8 * buf_n + 4, 4)
                .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?;
            let sub_buf_sz = LittleEndian::read_u32(&sub_buf_sz);
            if out_msg.len()
                + usize::try_from(sub_buf_sz).map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?
                >= 16 * 1024 * 1024
            {
                // TODO: arbitrary maximum message length
                panic!("Max message length reached");
                //return Err(());
            }
            out_msg.extend_from_slice(
                &thread
                    .read_memory(sub_buf_ptr, sub_buf_sz)
                    .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?,
            );
        }
        EncodedMessage(out_msg)
    };

    let needs_answer = params[3]
        .try_into::<i32>()
        .ok_or(ExtrinsicEmitMessageErr::BadParameter)?
        != 0;
    let allow_delay = params[4]
        .try_into::<i32>()
        .ok_or(ExtrinsicEmitMessageErr::BadParameter)?
        != 0;
    let message_id_write = if needs_answer {
        Some(
            u32::try_from(
                params[5]
                    .try_into::<i32>()
                    .ok_or(ExtrinsicEmitMessageErr::BadParameter)?,
            )
            .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?,
        )
    } else {
        None
    };

    Ok(EmitMessage {
        interface,
        message_id_write,
        message,
        allow_delay,
    })
}

/// Error that [`parse_extrinsic_emit_message`] can return.
#[derive(Debug)]
enum ExtrinsicEmitMessageErr {
    /// Bad type or invalid value for a parameter.
    BadParameter,
}

/// Analyzes a call to `emit_answer` made by the given thread.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
fn parse_extrinsic_emit_answer<TPud, TTud, TExtId>(
    thread: &mut processes::ProcessesCollectionThread<TPud, LocalThreadUserData<TTud, TExtId>>,
    params: Vec<wasmi::RuntimeValue>,
) -> Result<EmitAnswer, ExtrinsicEmitAnswerErr> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 3);

    let message_id = {
        let addr = u32::try_from(
            params[0]
                .try_into::<i32>()
                .ok_or(ExtrinsicEmitAnswerErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitAnswerErr::BadParameter)?;
        let buf = thread
            .read_memory(addr, 8)
            .map_err(|_| ExtrinsicEmitAnswerErr::BadParameter)?;
        MessageId::from(byteorder::LittleEndian::read_u64(&buf))
    };

    let response = {
        let addr = u32::try_from(
            params[1]
                .try_into::<i32>()
                .ok_or(ExtrinsicEmitAnswerErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitAnswerErr::BadParameter)?;
        let sz = u32::try_from(
            params[2]
                .try_into::<i32>()
                .ok_or(ExtrinsicEmitAnswerErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitAnswerErr::BadParameter)?;
        EncodedMessage(
            thread
                .read_memory(addr, sz)
                .map_err(|_| ExtrinsicEmitAnswerErr::BadParameter)?,
        )
    };

    Ok(EmitAnswer {
        message_id,
        response,
    })
}

/// Error that [`parse_extrinsic_emit_answer`] can return.
#[derive(Debug)]
enum ExtrinsicEmitAnswerErr {
    /// Bad type or invalid value for a parameter.
    BadParameter,
}

/// Analyzes a call to `emit_message_error` made by the given thread.
/// Returns the message for which to notify of an error.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
fn parse_extrinsic_emit_message_error<TPud, TTud, TExtId>(
    thread: &mut processes::ProcessesCollectionThread<TPud, LocalThreadUserData<TTud, TExtId>>,
    params: Vec<wasmi::RuntimeValue>,
) -> Result<MessageId, ExtrinsicEmitMessageErrorErr> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 1);

    let msg_id = {
        let addr = u32::try_from(
            params[0]
                .try_into::<i32>()
                .ok_or(ExtrinsicEmitMessageErrorErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitMessageErrorErr::BadParameter)?;
        let buf = thread
            .read_memory(addr, 8)
            .map_err(|_| ExtrinsicEmitMessageErrorErr::BadParameter)?;
        MessageId::from(byteorder::LittleEndian::read_u64(&buf))
    };

    Ok(msg_id)
}

/// Error that [`parse_extrinsic_emit_message_error`] can return.
#[derive(Debug)]
enum ExtrinsicEmitMessageErrorErr {
    /// Bad type or invalid value for a parameter.
    BadParameter,
}

/// Analyzes a call to `cancel_message` made by the given thread.
/// Returns the message to cancel.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
fn parse_extrinsic_cancel_message<TPud, TTud, TExtCtxt>(
    thread: &mut processes::ProcessesCollectionThread<TPud, LocalThreadUserData<TTud, TExtCtxt>>,
    params: Vec<wasmi::RuntimeValue>,
) -> Result<MessageId, ExtrinsicCancelMessageErr> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 1);

    let msg_id = {
        let addr = u32::try_from(
            params[0]
                .try_into::<i32>()
                .ok_or(ExtrinsicCancelMessageErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicCancelMessageErr::BadParameter)?;
        let buf = thread
            .read_memory(addr, 8)
            .map_err(|_| ExtrinsicCancelMessageErr::BadParameter)?;
        MessageId::from(byteorder::LittleEndian::read_u64(&buf))
    };

    Ok(msg_id)
}

/// Error that [`parse_extrinsic_cancel_message`] can return.
#[derive(Debug)]
enum ExtrinsicCancelMessageErr {
    /// Bad type or invalid value for a parameter.
    BadParameter,
}
