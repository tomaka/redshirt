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

use crate::module::Module;
use crate::scheduler::{processes, vm};
use crate::sig;
use crate::{InterfaceHash, MessageId};

use alloc::{rc::Rc, vec::Vec};
use core::{cell::RefCell, convert::TryFrom as _, fmt, mem};
use redshirt_syscalls::{EncodedMessage, Pid, ThreadId};

mod calls;

/// Wrapper around [`ProcessesCollection`](processes::ProcessesCollection), but that interprets
/// the extrinsic calls and keeps track of the state in which pending threads are in.
///
/// The generic parameters `TPud` and `TTud` are "user data"s that are stored respectively per
/// process and per thread, and allows the user to put extra information associated to a process
/// or a thread.
pub struct ProcessesCollectionExtrinsics<TPud, TTud> {
    inner: RefCell<
        processes::ProcessesCollection<
            Extrinsic,
            LocalProcessUserData<TPud>,
            LocalThreadUserData<TTud>,
        >,
    >,
    // TODO: implement
    /*/// List of processes that have died but that we haven't reported yet to the outside because
    /// they are locked.
    dead_processes: ,*/
}

/// Prototype for a `ProcessesCollectionExtrinsics` under construction.
pub struct ProcessesCollectionExtrinsicsBuilder {
    inner: processes::ProcessesCollectionBuilder<Extrinsic>,
}

/// Access to a process within the collection.
pub struct ProcessesCollectionExtrinsicsProc<'a, TPud, TTud> {
    parent: &'a ProcessesCollectionExtrinsics<TPud, TTud>,
    pid: Pid,
    user_data: Rc<TPud>,
}

/// Access to a thread within the collection that is in an interrupted state.
///
/// Implements the [`ProcessesCollectionExtrinsicsThreadAccess`] trait.
pub enum ProcessesCollectionExtrinsicsThread<'a, TPud, TTud> {
    EmitMessage(ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>),
    WaitNotification(ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud>),
}

/// Access to a thread within the collection.
///
/// Implements the [`ProcessesCollectionExtrinsicsThreadAccess`] trait.
pub struct ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud> {
    parent: &'a ProcessesCollectionExtrinsics<TPud, TTud>,
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
pub struct ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud> {
    parent: &'a ProcessesCollectionExtrinsics<TPud, TTud>,
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
enum Extrinsic {
    NextMessage,
    EmitMessage,
    EmitMessageError,
    EmitAnswer,
    CancelMessage,
}

/// Structure passed to the underlying [`processes::ProcessesCollection`] that tracks the state
/// of a process.
#[derive(Debug)]
struct LocalProcessUserData<TPud> {
    /// User data decided by the user.
    external_user_data: Rc<TPud>,
}

/// Structure passed to the underlying [`processes::ProcessesCollection`] that tracks the state
/// of a thread.
#[derive(Debug)]
struct LocalThreadUserData<TTud> {
    /// State of a thread.
    state: LocalThreadState,
    /// User data decided by the user. When the thread is locked, this user data is extracted
    /// and stored locally in the lock. The data is put back when the thread is unlocked.
    external_user_data: Option<TTud>,
}

/// State of a thread. Private. Stored within the [`processes::ProcessesCollection`].
#[derive(Debug)]
enum LocalThreadState {
    /// Thread is ready to run, running, or has just called an extrinsic and the call is being
    /// processed.
    ReadyToRun,

    /// The thread is sleeping and waiting for a notification to come.
    NotificationWait(calls::NotificationWait),

    /// The thread called `emit_message` and wants to emit a message on an interface.
    EmitMessage(calls::EmitMessage),
}

/// Outcome of the [`run`](ProcessesCollectionExtrinsics::run) function.
#[derive(Debug)]
pub enum RunOneOutcome<'a, TPud, TTud> {
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
        process: ProcessesCollectionExtrinsicsProc<'a, TPud, TTud>,

        /// User data of the thread.
        user_data: TTud,

        /// Value returned by the function that was executed.
        value: Option<wasmi::RuntimeValue>,
    },

    /// A thread in a process wants to emit a message.
    ThreadEmitMessage(ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>),

    /// A thread in a process is waiting for an incoming notification.
    ThreadWaitNotification(ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud>),

    /// A thread in a process wants to answer a message.
    ThreadEmitAnswer {
        /// Thread that wants to emit an answer.
        thread_id: ThreadId,

        /// Process that the thread belongs to.
        process: ProcessesCollectionExtrinsicsProc<'a, TPud, TTud>,

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
        process: ProcessesCollectionExtrinsicsProc<'a, TPud, TTud>,

        /// Message that is erroneous.
        message_id: MessageId,
    },

    /// A thread in a process wants to notify that a message is to be cancelled.
    ThreadCancelMessage {
        /// Thread that wants to emit a cancellation.
        thread_id: ThreadId,

        /// Process that the thread belongs to.
        process: ProcessesCollectionExtrinsicsProc<'a, TPud, TTud>,

        /// Message that must be cancelled.
        message_id: MessageId,
    },

    /// No thread is ready to run. Nothing was done.
    Idle,
}

impl<TPud, TTud> ProcessesCollectionExtrinsics<TPud, TTud> {
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
    ) -> Result<ProcessesCollectionExtrinsicsProc<TPud, TTud>, vm::NewErr> {
        let external_user_data = Rc::new(proc_user_data);
        let proc_user_data = LocalProcessUserData {
            external_user_data: external_user_data.clone(),
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
    pub fn run(&self) -> RunOneOutcome<TPud, TTud> {
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
                let next_msg = match calls::parse_extrinsic_next_notification(&mut thread, params) {
                    Ok(m) => m,
                    Err(_) => panic!(), // TODO:
                };
                thread.user_data().state = LocalThreadState::NotificationWait(next_msg);
                let process_user_data = thread.process().user_data().external_user_data.clone();
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
                let emit_msg = match calls::parse_extrinsic_emit_message(&mut thread, params) {
                    Ok(m) => m,
                    Err(_) => panic!(), // TODO:
                };
                thread.user_data().state = LocalThreadState::EmitMessage(emit_msg);
                let process_user_data = thread.process().user_data().external_user_data.clone();
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
                let emit_resp = match calls::parse_extrinsic_emit_answer(&mut thread, params) {
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
                let emit_msg_error =
                    match calls::parse_extrinsic_emit_message_error(&mut thread, params) {
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
                let emit_cancel = match calls::parse_extrinsic_cancel_message(&mut thread, params) {
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
    pub fn process_by_id(&self, pid: Pid) -> Option<ProcessesCollectionExtrinsicsProc<TPud, TTud>> {
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
    ) -> Result<ProcessesCollectionExtrinsicsThread<TPud, TTud>, ThreadByIdErr> {
        let mut inner = self.inner.borrow_mut();
        let mut inner = inner
            .interrupted_thread_by_id(id)
            .ok_or(ThreadByIdErr::RunningOrDead)?;  // TODO: might now also be already locked

        // Checking thread locked state.
        debug_assert!(inner.user_data().external_user_data.is_some());

        match inner.user_data().state {
            LocalThreadState::ReadyToRun => unreachable!(),
            LocalThreadState::EmitMessage(_) => {
                let process_user_data = inner.process().user_data().external_user_data.clone();
                let thread_user_data = inner.user_data().external_user_data.take().unwrap();

                Ok(From::from(ProcessesCollectionExtrinsicsThreadEmitMessage {
                    parent: self,
                    tid: id,
                    process_user_data,
                    thread_user_data: Some(thread_user_data),
                }))
            }
            LocalThreadState::NotificationWait(_) => {
                let process_user_data = inner.process().user_data().external_user_data.clone();
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

impl Default for ProcessesCollectionExtrinsicsBuilder {
    fn default() -> ProcessesCollectionExtrinsicsBuilder {
        let inner = processes::ProcessesCollectionBuilder::default()
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

        ProcessesCollectionExtrinsicsBuilder { inner }
    }
}

impl ProcessesCollectionExtrinsicsBuilder {
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
    pub fn build<TPud, TTud>(self) -> ProcessesCollectionExtrinsics<TPud, TTud> {
        ProcessesCollectionExtrinsics {
            inner: RefCell::new(self.inner.build()),
        }
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsProc<'a, TPud, TTud> {
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
    ) -> impl Iterator<Item = ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>> {
        let mut inner = self.parent.inner.borrow_mut();
        let inner = inner.process_by_id(self.pid).unwrap();
        // TODO: let thread = inner.interrupted_threads();

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

impl<'a, TPud, TTud> fmt::Debug for ProcessesCollectionExtrinsicsProc<'a, TPud, TTud>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: improve
        f.debug_tuple("ProcessesCollectionExtrinsicsProc").finish()
    }
}

impl<'a, TPud, TTud> From<ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>>
    for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>
{
    fn from(thread: ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>) -> Self {
        ProcessesCollectionExtrinsicsThread::EmitMessage(thread)
    }
}

impl<'a, TPud, TTud> From<ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud>>
    for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>
{
    fn from(thread: ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud>) -> Self {
        ProcessesCollectionExtrinsicsThread::WaitNotification(thread)
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThreadAccess<'a>
    for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>
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

impl<'a, TPud, TTud> fmt::Debug for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => fmt::Debug::fmt(t, f),
            ProcessesCollectionExtrinsicsThread::WaitNotification(t) => fmt::Debug::fmt(t, f),
        }
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud> {
    /// Returns true if the caller wants an answer to the message.
    pub fn needs_answer(&mut self) -> bool {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();

        if let LocalThreadState::EmitMessage(ref emit) = inner.user_data().state {
            emit.message_id_write.is_some()
        } else {
            unreachable!()
        }
    }

    /// Returns the interface to emit the message on.
    pub fn emit_interface(&mut self) -> InterfaceHash {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();

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
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();

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
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();

        let process = inner.process();

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

            process
                .write_memory(message_id_write, &u64::from(message_id).to_le_bytes())
                .unwrap();
        } else {
            assert!(message_id.is_none());
        }

        inner.resume(Some(wasmi::RuntimeValue::I32(0)));
        emit.message
    }

    /// Resumes the thread, signalling an error in the emission.
    pub fn refuse_emit(self) {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();
        inner.resume(Some(wasmi::RuntimeValue::I32(1)));
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThreadAccess<'a>
    for ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&mut self) -> ThreadId {
        self.tid
    }

    fn pid(&self) -> Pid {
        let mut inner = self.parent.inner.borrow_mut();
        inner.interrupted_thread_by_id(self.tid).unwrap().pid()
    }

    fn process_user_data(&self) -> &TPud {
        &self.process_user_data
    }

    fn user_data(&mut self) -> &mut TTud {
        self.thread_user_data.as_mut().unwrap()
    }
}

impl<'a, TPud, TTud> Drop for ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud> {
    fn drop(&mut self) {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();
        let external_user_data = &mut inner.user_data().external_user_data;
        debug_assert!(external_user_data.is_none());
        *external_user_data = Some(self.thread_user_data.take().unwrap());
    }
}

impl<'a, TPud, TTud> fmt::Debug for ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>
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

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud> {
    /// Returns the list of message IDs that the thread is waiting on. In order.
    // TODO: not great naming. we're waiting either for messages or an interface notif or a process cancelled notif
    pub fn message_ids_iter<'b>(&'b mut self) -> impl Iterator<Item = MessageId> + 'b {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();

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
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();

        if let LocalThreadState::NotificationWait(ref wait) = inner.user_data().state {
            usize::try_from(wait.out_size).unwrap()
        } else {
            unreachable!()
        }
    }

    /// Returns true if we should block the thread waiting for a notification to come.
    pub fn block(&mut self) -> bool {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();

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
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();

        let wait = {
            match mem::replace(&mut inner.user_data().state, LocalThreadState::ReadyToRun) {
                LocalThreadState::NotificationWait(wait) => wait,
                _ => unreachable!(),
            }
        };

        assert!(index < wait.notifs_ids.len());
        let notif_size_u32 = u32::try_from(notif.0.len()).unwrap();
        assert!(wait.out_size >= notif_size_u32);

        let process = inner.process();

        // Write the notification in the process's memory.
        match process.write_memory(wait.out_pointer, &notif.0) {
            Ok(()) => {}
            Err(_) => panic!(), // TODO: can legit happen
        };

        // Zero the corresponding entry in the notifications to wait upon.
        match process.write_memory(
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
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();

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
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();

        if let LocalThreadState::NotificationWait(ref wait) = inner.user_data().state {
            assert!(!wait.block);
        } else {
            unreachable!()
        }

        inner.user_data().state = LocalThreadState::ReadyToRun;
        inner.resume(Some(wasmi::RuntimeValue::I32(0)));
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThreadAccess<'a>
    for ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&mut self) -> ThreadId {
        self.tid
    }

    fn pid(&self) -> Pid {
        let mut inner = self.parent.inner.borrow_mut();
        inner.interrupted_thread_by_id(self.tid).unwrap().pid()
    }

    fn process_user_data(&self) -> &TPud {
        &self.process_user_data
    }

    fn user_data(&mut self) -> &mut TTud {
        self.thread_user_data.as_mut().unwrap()
    }
}

impl<'a, TPud, TTud> Drop for ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud> {
    fn drop(&mut self) {
        let mut inner = self.parent.inner.borrow_mut();
        let mut inner = inner.interrupted_thread_by_id(self.tid).unwrap();
        let external_user_data = &mut inner.user_data().external_user_data;
        debug_assert!(external_user_data.is_none());
        *external_user_data = Some(self.thread_user_data.take().unwrap());
    }
}

impl<'a, TPud, TTud> fmt::Debug
    for ProcessesCollectionExtrinsicsThreadWaitNotification<'a, TPud, TTud>
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

impl LocalThreadState {
    /// True if `self` is equal to [`LocalThreadState::ReadyToRun`].
    fn is_ready_to_run(&self) -> bool {
        match self {
            LocalThreadState::ReadyToRun => true,
            _ => false,
        }
    }
}
