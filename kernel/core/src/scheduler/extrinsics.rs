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

//! Collection of VMs representing processes.
//!
//! This module wraps around a [`processes::ProcessesCollection`]. The documentation of the
//! [`processes`] module also applies to this module and it is strongly recommended it read it
//! first.
//!
//! On top of the features of the [`processes`] module, this module also handles the logic related
//! to extrinsics (in other words, functions that the Wasm modules can import and call).
//!
//! In terms of API, the changes compared to [`processes`] are:
//!
//! - The collection accepts an extra generic parameter that must implement the [`Extrinsics`]
//! trait. Implementations of this trait can provide support for additional functions callable by
//! the Wasm module by translating them into messages.
//!
//! - Interrupted threads are more strongly typed and are split into two categories: threads that
//! are interrupted because they want to emit a message, and threads that are interrupted because
//! they are waiting for a notification.

use crate::extrinsics::{
    Extrinsics, ExtrinsicsAction, ExtrinsicsMemoryAccess, ExtrinsicsMemoryAccessErr,
};
use crate::scheduler::{processes, vm};
use crate::sig;
use crate::{InterfaceHash, MessageId};

use alloc::vec::Vec;
use core::{convert::TryFrom as _, fmt, iter, mem, ops::Range};
use crossbeam_queue::SegQueue;
use redshirt_syscalls::{EncodedMessage, Pid, ThreadId};

mod calls;

pub use calls::WaitEntry;
pub use processes::Trap; // TODO: redefine locally?

/// Wrapper around [`ProcessesCollection`](processes::ProcessesCollection), but that interprets
/// the extrinsic calls and keeps track of the state in which pending threads are in.
///
/// The generic parameters `TPud` and `TTud` are "user data"s that are stored respectively per
/// process and per thread, and allows the user to put extra information associated to a process
/// or a thread.
pub struct ProcessesCollectionExtrinsics<TPud, TTud, TExt: Extrinsics> {
    inner: processes::ProcessesCollection<
        Extrinsic<TExt::ExtrinsicId>,
        LocalProcessUserData<TPud, TExt>,
        LocalThreadUserData<TTud, TExt::Context>,
    >,

    /// List of threads that `inner` considers "interrupted" but that we expose as "ready". We
    /// have to process the external extrinsics for this thread.
    ///
    /// The threads here must always be in the [`LocalThreadState::OtherExtrinsicApplyAction`]
    /// or the [`LocalThreadState::OtherExtrinsicReportWait`] state.
    // TODO: we have to notify wakers when we push an element
    local_run_queue: SegQueue<ThreadId>,
}

/// Prototype for a `ProcessesCollectionExtrinsics` under construction.
pub struct Builder<TExt: Extrinsics> {
    inner: processes::ProcessesCollectionBuilder<Extrinsic<TExt::ExtrinsicId>>,
}

/// Access to a process within the collection.
pub struct ProcAccess<'a, TPud, TTud, TExt: Extrinsics> {
    parent: &'a ProcessesCollectionExtrinsics<TPud, TTud, TExt>,
    inner: processes::ProcAccess<
        'a,
        Extrinsic<TExt::ExtrinsicId>,
        LocalProcessUserData<TPud, TExt>,
        LocalThreadUserData<TTud, TExt::Context>,
    >,
}

/// Access to a thread within the collection that is in an interrupted state.
///
/// Implements the [`ThreadAccessAccess`] trait.
pub enum ThreadAccess<'a, TPud, TTud, TExt: Extrinsics> {
    EmitMessage(ThreadEmitMessage<'a, TPud, TTud, TExt>),
    WaitNotification(ThreadWaitNotif<'a, TPud, TTud, TExt>),
}

/// Access to a thread within the collection.
///
/// Implements the [`ThreadAccessAccess`] trait.
pub struct ThreadEmitMessage<'a, TPud, TTud, TExt: Extrinsics> {
    process: ProcAccess<'a, TPud, TTud, TExt>,
    inner: processes::ThreadAccess<
        'a,
        Extrinsic<TExt::ExtrinsicId>,
        LocalProcessUserData<TPud, TExt>,
        LocalThreadUserData<TTud, TExt::Context>,
    >,
}

/// Access to a thread within the collection.
///
/// Implements the [`ThreadAccessAccess`] trait.
pub struct ThreadWaitNotif<'a, TPud, TTud, TExt: Extrinsics> {
    process: ProcAccess<'a, TPud, TTud, TExt>,
    inner: processes::ThreadAccess<
        'a,
        Extrinsic<TExt::ExtrinsicId>,
        LocalProcessUserData<TPud, TExt>,
        LocalThreadUserData<TTud, TExt::Context>,
    >,
}

/// Common trait amongst all the thread accessor structs.
pub trait ThreadAccessAccess<'a> {
    type ProcessUserData;
    type ThreadUserData;

    // TODO: make it return handle to process instead?

    /// Returns the id of the thread. Allows later retrieval by calling
    /// [`interrupted_thread_by_id`](ProcessesCollectionExtrinsics::interrupted_thread_by_id).
    ///
    /// [`ThreadId`]s are unique within a [`ProcessesCollectionExtrinsics`], independently from the
    /// process.
    fn tid(&self) -> ThreadId;

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
    CancelMessage,
    Other(TExtId),
}

/// Structure passed to the underlying [`processes::ProcessesCollection`] that tracks the state
/// of a process.
#[derive(Debug)]
struct LocalProcessUserData<TPud, TExt> {
    /// User data decided by the user.
    external_user_data: TPud,
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
    external_user_data: TTud,
}

/// State of a thread. Private. Stored within the [`processes::ProcessesCollection`].
#[derive(Debug)]
enum LocalThreadState<TExtCtxt> {
    /// Thread is ready to run, running, has just called an extrinsic and the call is being
    /// processed, or has deliberately been put in limbo before the process is being aborted.
    ReadyToRun,

    /// Thread is in the middle of a non-hardcoded extrinsic. We now need to apply the given
    /// action on the context.
    ///
    /// Threads in this state must be pushed to [`ProcessesCollectionExtrinsics::local_run_queue`].
    OtherExtrinsicApplyAction {
        /// Abstract context used to drive the extrinsic call.
        context: TExtCtxt,
        /// Action to perform.
        action: ExtrinsicsAction,
    },

    /// Thread is running a non-hardcoded extrinsic that wants to emit a message.
    OtherExtrinsicEmit {
        /// Abstract context used to drive the extrinsic call.
        context: TExtCtxt,
        /// Interface to emit the message on.
        interface: InterfaceHash,
        /// Message to emit.
        message: EncodedMessage,
        /// True if a message is expected.
        response_expected: bool,
    },

    /// Thread must be reported as a waiting thread through the API, then transition to
    /// [`LocalThreadState::OtherExtrinsicWait`].
    OtherExtrinsicReportWait {
        /// Abstract context used to drive the extrinsic call.
        context: TExtCtxt,
        /// Message for which we are awaiting a response.
        message: MessageId,
    },

    /// Thread is running a non-hardcoded extrinsic waiting for a response.
    OtherExtrinsicWait {
        /// Abstract context used to drive the extrinsic call.
        context: TExtCtxt,
        /// Message for which we are awaiting a response.
        message: MessageId,
    },

    /// The thread is sleeping and waiting for a notification to come.
    NotificationWait(calls::NotificationWait),

    /// The thread called `emit_message` and wants to emit a message on an interface.
    EmitMessage(calls::EmitMessage),

    /// Temporary state while we move things around. If encountered unexpectedly, that indicates
    /// a bug in the code.
    Poisoned,
}

/// Event returned by [`ProcessesCollectionExtrinsics::run`].
pub enum ExecuteOut<'a, TPud, TTud, TExt: Extrinsics> {
    /// Event directly generated.
    Direct(RunOneOutcome<'a, TPud, TTud, TExt>),
    /// Ready to execute a bit of a thread.
    ReadyToRun(ReadyToRun<'a, TPud, TTud, TExt>),
}

/// Ready to resume one of the threads of a process.
#[must_use]
pub struct ReadyToRun<'a, TPud, TTud, TExt: Extrinsics> {
    collection: &'a ProcessesCollectionExtrinsics<TPud, TTud, TExt>,
    inner: processes::ReadyToRun<
        'a,
        Extrinsic<TExt::ExtrinsicId>,
        LocalProcessUserData<TPud, TExt>,
        LocalThreadUserData<TTud, TExt::Context>,
    >,
}

impl<'a, TPud, TTud, TExt: Extrinsics> ReadyToRun<'a, TPud, TTud, TExt> {
    /// Performs the actual execution.
    ///
    /// Returns `None` if the execution doesn't lead to any event in particular.
    pub fn run(mut self) -> Option<RunOneOutcome<'a, TPud, TTud, TExt>> {
        self.collection.inner_event(self.inner.run())
    }
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
        outcome: Result<Option<crate::WasmValue>, Trap>,
    },

    /// A thread in a process has finished.
    ThreadFinished {
        /// Thread which has finished.
        thread_id: ThreadId,

        /// Process whose thread has finished.
        process: ProcAccess<'a, TPud, TTud, TExt>,

        /// User data of the thread.
        user_data: TTud,

        /// Value returned by the function that was executed.
        value: Option<crate::WasmValue>,
    },

    /// A thread in a process wants to emit a message.
    ThreadEmitMessage(ThreadEmitMessage<'a, TPud, TTud, TExt>),

    /// A thread in a process is waiting for an incoming message.
    ThreadWaitNotification(ThreadWaitNotif<'a, TPud, TTud, TExt>),

    /// A thread in a process wants to notify that a message is to be cancelled.
    ThreadCancelMessage {
        /// Thread that wants to emit a cancellation.
        thread_id: ThreadId,

        /// Process that the thread belongs to.
        process: ProcAccess<'a, TPud, TTud, TExt>,

        /// Message that must be cancelled.
        message_id: MessageId,
    },
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
        module: &[u8],
        proc_user_data: TPud,
        main_thread_user_data: TTud,
    ) -> Result<(ProcAccess<TPud, TTud, TExt>, ThreadId), vm::NewErr> {
        let proc_user_data = LocalProcessUserData {
            extrinsics: Default::default(),
            external_user_data: proc_user_data,
        };
        let main_thread_user_data = LocalThreadUserData {
            state: LocalThreadState::ReadyToRun,
            external_user_data: main_thread_user_data,
        };
        let (inner, main_tid) =
            self.inner
                .execute(module, proc_user_data, main_thread_user_data)?;
        Ok((
            ProcAccess {
                parent: self,
                inner,
            },
            main_tid,
        ))
    }

    /// Runs one thread amongst the collection.
    ///
    /// Which thread is run is implementation-defined and no guarantee is made.
    pub async fn run<'a>(&'a self) -> ExecuteOut<'a, TPud, TTud, TExt> {
        loop {
            while let Some(tid) = self.local_run_queue.pop() {
                // It is possible that the thread no longer exists, for example if the process crashed.
                let mut thread = match self.inner.interrupted_thread_by_id(tid) {
                    Some(t) => t,
                    None => continue,
                };

                match mem::replace(
                    &mut thread.user_data_mut().state,
                    LocalThreadState::Poisoned,
                ) {
                    LocalThreadState::OtherExtrinsicReportWait { context, message } => {
                        thread.user_data_mut().state =
                            LocalThreadState::OtherExtrinsicWait { context, message };
                        let process = ProcAccess {
                            parent: self,
                            inner: thread.process(),
                        };
                        return ExecuteOut::Direct(RunOneOutcome::ThreadWaitNotification(
                            ThreadWaitNotif {
                                process,
                                inner: thread,
                            },
                        ));
                    }
                    LocalThreadState::OtherExtrinsicApplyAction { context, action } => match action
                    {
                        ExtrinsicsAction::ProgramCrash => unimplemented!(),
                        ExtrinsicsAction::Resume(value) => {
                            thread.user_data_mut().state = LocalThreadState::ReadyToRun;
                            thread.resume(value)
                        }
                        ExtrinsicsAction::EmitMessage {
                            interface,
                            message,
                            response_expected,
                        } => {
                            thread.user_data_mut().state = LocalThreadState::OtherExtrinsicEmit {
                                context,
                                interface,
                                message,
                                response_expected,
                            };
                            let process = ProcAccess {
                                parent: self,
                                inner: thread.process(),
                            };
                            return ExecuteOut::Direct(RunOneOutcome::ThreadEmitMessage(
                                ThreadEmitMessage {
                                    process,
                                    inner: thread,
                                },
                            ));
                        }
                    },
                    _ => unreachable!(),
                }
            }

            match self.inner.run().await {
                processes::RunFutureOut::Direct(ev) => {
                    if let Some(ev) = self.inner_event(ev) {
                        return ExecuteOut::Direct(ev);
                    }
                }
                processes::RunFutureOut::ReadyToRun(inner) => {
                    return ExecuteOut::ReadyToRun(ReadyToRun {
                        collection: self,
                        inner,
                    })
                }
            }
        }
    }

    fn inner_event<'a>(
        &'a self,
        mut outcome: processes::RunOneOutcome<
            'a,
            Extrinsic<TExt::ExtrinsicId>,
            LocalProcessUserData<TPud, TExt>,
            LocalThreadUserData<TTud, TExt::Context>,
        >,
    ) -> Option<RunOneOutcome<'a, TPud, TTud, TExt>> {
        match outcome {
            processes::RunOneOutcome::ProcessFinished {
                pid,
                user_data,
                dead_threads,
                outcome,
            } => {
                Some(RunOneOutcome::ProcessFinished {
                    pid,
                    user_data: user_data.external_user_data,
                    dead_threads: dead_threads
                        .into_iter()
                        .map(|(id, state)| (id, state.external_user_data))
                        .collect(), // TODO: meh for allocation
                    outcome,
                })
            }

            processes::RunOneOutcome::StartProcessAbort { .. } => None,

            processes::RunOneOutcome::ThreadFinished {
                process,
                user_data,
                value,
                thread_id,
            } => {
                debug_assert!(user_data.state.is_ready_to_run());
                Some(RunOneOutcome::ThreadFinished {
                    thread_id,
                    process: ProcAccess {
                        parent: self,
                        inner: process,
                    },
                    user_data: user_data.external_user_data,
                    value,
                })
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::NextMessage,
                params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                match calls::parse_extrinsic_next_notification(&mut thread, params) {
                    Ok(next_msg) => {
                        thread.user_data_mut().state = LocalThreadState::NotificationWait(next_msg);
                        let process = ProcAccess {
                            parent: self,
                            inner: thread.process(),
                        };
                        Some(RunOneOutcome::ThreadWaitNotification(ThreadWaitNotif {
                            process,
                            inner: thread,
                        }))
                    }
                    Err(_) => {
                        thread.process().abort();
                        None
                    }
                }
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::EmitMessage,
                params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                match calls::parse_extrinsic_emit_message(&mut thread, params) {
                    Ok(emit_msg) => {
                        thread.user_data_mut().state = LocalThreadState::EmitMessage(emit_msg);
                        let process = ProcAccess {
                            parent: self,
                            inner: thread.process(),
                        };
                        Some(RunOneOutcome::ThreadEmitMessage(ThreadEmitMessage {
                            process,
                            inner: thread,
                        }))
                    }
                    Err(_) => {
                        thread.process().abort();
                        None
                    }
                }
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::CancelMessage,
                params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                match calls::parse_extrinsic_cancel_message(&mut thread, params) {
                    Ok(emit_cancel) => {
                        let process = thread.process();
                        let thread_id = thread.tid();
                        thread.resume(None);
                        Some(RunOneOutcome::ThreadCancelMessage {
                            process: ProcAccess {
                                parent: self,
                                inner: process,
                            },
                            thread_id,
                            message_id: emit_cancel,
                        })
                    }
                    Err(_) => {
                        thread.process().abort();
                        None
                    }
                }
            }

            processes::RunOneOutcome::Interrupted {
                ref mut thread,
                id: Extrinsic::Other(ext_id),
                ref params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                let thread_id = thread.tid();
                let (context, action) = thread.process().user_data().extrinsics.new_context(
                    thread_id,
                    ext_id,
                    params.iter().cloned(),
                    &mut MemoryAccessImpl(thread),
                );
                thread.user_data_mut().state =
                    LocalThreadState::OtherExtrinsicApplyAction { context, action };
                self.local_run_queue.push(thread_id);
                None
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
    pub fn process_by_id(&self, pid: Pid) -> Option<ProcAccess<TPud, TTud, TExt>> {
        Some(ProcAccess {
            parent: self,
            inner: self.inner.process_by_id(pid)?,
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
    ) -> Result<ThreadAccess<TPud, TTud, TExt>, ThreadByIdErr> {
        let inner = self
            .inner
            .interrupted_thread_by_id(id)
            .ok_or(ThreadByIdErr::RunningOrDead)?;

        match inner.user_data().state {
            LocalThreadState::ReadyToRun => {
                // TODO: I'm a bit tired while writing this and not sure that's correct
                unreachable!()
            }
            LocalThreadState::OtherExtrinsicApplyAction { .. }
            | LocalThreadState::OtherExtrinsicReportWait { .. } => {
                Err(ThreadByIdErr::RunningOrDead)
            }
            LocalThreadState::EmitMessage(_) | LocalThreadState::OtherExtrinsicEmit { .. } => {
                let process = ProcAccess {
                    parent: self,
                    inner: inner.process(),
                };
                Ok(From::from(ThreadEmitMessage { process, inner }))
            }
            LocalThreadState::NotificationWait(_) | LocalThreadState::OtherExtrinsicWait { .. } => {
                let process = ProcAccess {
                    parent: self,
                    inner: inner.process(),
                };
                Ok(From::from(ThreadWaitNotif { process, inner }))
            }
            LocalThreadState::Poisoned => panic!(),
        }
    }
}

impl<TExt> Builder<TExt>
where
    TExt: Extrinsics,
{
    /// Initializes a new builder using the given random seed.
    ///
    /// The seed is used in determine how [`Pid`]s are generated. The same seed will result in
    /// the same sequence of [`Pid`]s.
    pub fn with_seed(seed: [u8; 32]) -> Self {
        let mut inner = processes::ProcessesCollectionBuilder::with_seed(seed)
            .with_extrinsic(
                "redshirt",
                "next_notification",
                sig!((I32, I32, I32, I32, I64) -> I32),
                Extrinsic::NextMessage,
            )
            .with_extrinsic(
                "redshirt",
                "emit_message",
                sig!((I32, I32, I32, I64, I32) -> I32),
                Extrinsic::EmitMessage,
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

        Builder { inner }
    }

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
            inner: self.inner.build(),
            local_run_queue: SegQueue::new(),
        }
    }
}

impl<'a, TPud, TTud, TExt> ProcAccess<'a, TPud, TTud, TExt>
where
    TExt: Extrinsics,
{
    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollectionExtrinsics::process_by_id).
    pub fn pid(&self) -> Pid {
        self.inner.pid()
    }

    /// Returns the user data that is associated to the process.
    pub fn user_data(&self) -> &TPud {
        &self.inner.user_data().external_user_data
    }

    /// Adds a new thread to the process, starting the function with the given index and passing
    /// the given parameters.
    ///
    /// > **Note**: The "function ID" is the index of the function in the WASM module. WASM
    /// >           doesn't have function pointers. Instead, all the functions are part of a single
    /// >           global array of functions.
    pub fn start_thread(
        &self,
        fn_index: u32,
        params: Vec<crate::WasmValue>,
        user_data: TTud,
    ) -> Result<ThreadId, vm::ThreadStartErr> {
        self.inner.start_thread(
            fn_index,
            params,
            LocalThreadUserData {
                state: LocalThreadState::ReadyToRun,
                external_user_data: user_data,
            },
        )
    }

    /// Marks the process as aborting.
    ///
    /// The termination will happen after all locks to this process have been released.
    ///
    /// Calling [`abort`](ProcAccess::abort) a second time or more has no
    /// effect.
    pub fn abort(&self) {
        self.inner.abort();
    }
}

impl<'a, TPud, TTud, TExt> fmt::Debug for ProcAccess<'a, TPud, TTud, TExt>
where
    TExt: Extrinsics + fmt::Debug,
    TPud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> From<ThreadEmitMessage<'a, TPud, TTud, TExt>>
    for ThreadAccess<'a, TPud, TTud, TExt>
{
    fn from(thread: ThreadEmitMessage<'a, TPud, TTud, TExt>) -> Self {
        ThreadAccess::EmitMessage(thread)
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> From<ThreadWaitNotif<'a, TPud, TTud, TExt>>
    for ThreadAccess<'a, TPud, TTud, TExt>
{
    fn from(thread: ThreadWaitNotif<'a, TPud, TTud, TExt>) -> Self {
        ThreadAccess::WaitNotification(thread)
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> ThreadAccessAccess<'a>
    for ThreadAccess<'a, TPud, TTud, TExt>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&self) -> ThreadId {
        match self {
            ThreadAccess::EmitMessage(t) => t.tid(),
            ThreadAccess::WaitNotification(t) => t.tid(),
        }
    }

    fn pid(&self) -> Pid {
        match self {
            ThreadAccess::EmitMessage(t) => t.pid(),
            ThreadAccess::WaitNotification(t) => t.pid(),
        }
    }

    fn process_user_data(&self) -> &TPud {
        match self {
            ThreadAccess::EmitMessage(t) => t.process_user_data(),
            ThreadAccess::WaitNotification(t) => t.process_user_data(),
        }
    }

    fn user_data(&mut self) -> &mut TTud {
        match self {
            ThreadAccess::EmitMessage(t) => t.user_data(),
            ThreadAccess::WaitNotification(t) => t.user_data(),
        }
    }
}

impl<'a, TPud, TTud, TExt> fmt::Debug for ThreadAccess<'a, TPud, TTud, TExt>
where
    TExt: Extrinsics,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ThreadAccess::EmitMessage(t) => fmt::Debug::fmt(t, f),
            ThreadAccess::WaitNotification(t) => fmt::Debug::fmt(t, f),
        }
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> ThreadEmitMessage<'a, TPud, TTud, TExt> {
    /// Returns true if the caller wants an answer to the message.
    pub fn needs_answer(&mut self) -> bool {
        match self.inner.user_data().state {
            LocalThreadState::EmitMessage(ref emit) => emit.message_id_write.is_some(),
            LocalThreadState::OtherExtrinsicEmit {
                response_expected, ..
            } => response_expected,
            _ => unreachable!(),
        }
    }

    /// Returns the interface to emit the message on.
    pub fn emit_interface(&mut self) -> &InterfaceHash {
        match self.inner.user_data().state {
            LocalThreadState::EmitMessage(ref emit) => &emit.interface,
            LocalThreadState::OtherExtrinsicEmit { ref interface, .. } => interface,
            _ => unreachable!(),
        }
    }

    /// True if the caller allows delays.
    pub fn allow_delay(&mut self) -> bool {
        match self.inner.user_data().state {
            LocalThreadState::EmitMessage(ref emit) => emit.allow_delay,
            LocalThreadState::OtherExtrinsicEmit { .. } => true,
            _ => unreachable!(),
        }
    }

    /// Returns the message to emit and resumes the thread.
    ///
    /// # Panic
    ///
    /// - Panics if `message_id.is_some() != thread.needs_answer()`. In other words, if
    /// `needs_answer` is true, then you **must** provide a `MessageId`.
    ///
    pub fn accept_emit(mut self, message_id: Option<MessageId>) -> EncodedMessage {
        match mem::replace(
            &mut self.inner.user_data_mut().state,
            LocalThreadState::Poisoned,
        ) {
            LocalThreadState::EmitMessage(emit) => {
                if let Some(message_id_write) = emit.message_id_write {
                    let message_id = match message_id {
                        Some(m) => m,
                        None => panic!(),
                    };

                    self.inner
                        .write_memory(message_id_write, &u64::from(message_id).to_le_bytes())
                        .unwrap();
                } else {
                    assert!(message_id.is_none());
                }

                self.inner.user_data_mut().state = LocalThreadState::ReadyToRun;
                self.inner.resume(Some(crate::WasmValue::I32(0)));
                emit.message
            }
            LocalThreadState::OtherExtrinsicEmit {
                mut context,
                message,
                response_expected,
                ..
            } => {
                if response_expected {
                    let message_id = message_id.unwrap();
                    self.inner.user_data_mut().state = LocalThreadState::OtherExtrinsicReportWait {
                        context,
                        message: message_id,
                    };
                    self.process.parent.local_run_queue.push(self.inner.tid());
                } else {
                    debug_assert!(message_id.is_none());
                    let action = self
                        .inner
                        .process()
                        .user_data()
                        .extrinsics
                        .inject_message_response(
                            &mut context,
                            None,
                            &mut MemoryAccessImpl(&mut self.inner),
                        );
                    self.inner.user_data_mut().state =
                        LocalThreadState::OtherExtrinsicApplyAction { context, action };
                    self.process.parent.local_run_queue.push(self.inner.tid());
                }

                message
            }
            _ => unreachable!(),
        }
    }

    /// Resumes the thread, signalling an error in the emission.
    pub fn refuse_emit(mut self) {
        match mem::replace(
            &mut self.inner.user_data_mut().state,
            LocalThreadState::Poisoned,
        ) {
            LocalThreadState::EmitMessage(_) => {
                self.inner.user_data_mut().state = LocalThreadState::ReadyToRun;
                self.inner.resume(Some(crate::WasmValue::I32(1)));
            }
            LocalThreadState::OtherExtrinsicEmit { context, .. } => {
                // TODO: don't know what else to do here than crash the program
                self.inner.user_data_mut().state = LocalThreadState::OtherExtrinsicApplyAction {
                    context,
                    action: ExtrinsicsAction::ProgramCrash,
                };
                self.process.parent.local_run_queue.push(self.inner.tid());
            }
            _ => unreachable!(),
        }
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> ThreadAccessAccess<'a>
    for ThreadEmitMessage<'a, TPud, TTud, TExt>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&self) -> ThreadId {
        self.inner.tid()
    }

    fn pid(&self) -> Pid {
        self.inner.process().pid()
    }

    fn process_user_data(&self) -> &TPud {
        self.process.user_data()
    }

    fn user_data(&mut self) -> &mut TTud {
        &mut self.inner.user_data_mut().external_user_data
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> fmt::Debug for ThreadEmitMessage<'a, TPud, TTud, TExt> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> ThreadWaitNotif<'a, TPud, TTud, TExt> {
    /// Unlocks the thread and returns the process it belongs to.
    pub fn into_process(self) -> ProcAccess<'a, TPud, TTud, TExt> {
        self.process
    }

    /// Returns the list of notifications that the thread is waiting on. In order, and preserving
    /// empty entries.
    pub fn wait_entries<'b>(&'b mut self) -> impl Iterator<Item = WaitEntry> + 'b {
        match self.inner.user_data().state {
            LocalThreadState::NotificationWait(ref wait) => {
                either::Either::Left(wait.notifs_ids.iter().cloned())
            }
            LocalThreadState::OtherExtrinsicWait { message, .. } => {
                either::Either::Right(iter::once(WaitEntry::Answer(message)))
            }
            _ => unreachable!(),
        }
    }

    /// Returns the maximum size allowed for a notification.
    pub fn allowed_notification_size(&self) -> usize {
        match self.inner.user_data().state {
            LocalThreadState::NotificationWait(ref wait) => usize::try_from(wait.out_size).unwrap(),
            LocalThreadState::OtherExtrinsicWait { .. } => usize::max_value(),
            _ => unreachable!(),
        }
    }

    /// Returns true if we should block the thread waiting for a notification to come.
    pub fn block(&self) -> bool {
        match self.inner.user_data().state {
            LocalThreadState::NotificationWait(ref wait) => wait.block,
            LocalThreadState::OtherExtrinsicWait { .. } => true,
            _ => unreachable!(),
        }
    }

    /// Resume the thread, sending back a notification.
    ///
    /// `index` must be the index within the list returned by
    /// [`wait_entries`](ThreadWaitNotif::wait_entries).
    ///
    /// # Panic
    ///
    /// - Panics if the notification is too large. You should make sure this is not the case before
    /// calling this function.
    /// - Panics if `index` is too large.
    ///
    pub fn resume_notification(mut self, index: usize, notif: EncodedMessage) {
        match mem::replace(
            &mut self.inner.user_data_mut().state,
            LocalThreadState::Poisoned,
        ) {
            LocalThreadState::NotificationWait(wait) => {
                debug_assert!(index < wait.notifs_ids.len());
                assert_ne!(wait.notifs_ids[index], WaitEntry::Empty);
                let notif_size_u32 = u32::try_from(notif.0.len()).unwrap();
                assert!(wait.out_size >= notif_size_u32);

                self.inner.user_data_mut().state = LocalThreadState::ReadyToRun;

                // Write the notification in the process's memory.
                match self.inner.write_memory(wait.out_pointer, &notif.0) {
                    Ok(()) => {}
                    Err(_) => {
                        self.inner.process().abort();
                        return;
                    }
                };

                // Zero the corresponding entry in the notifications to wait upon.
                match self.inner.write_memory(
                    wait.notifs_ids_ptr + u32::try_from(index).unwrap() * 8,
                    &[0; 8],
                ) {
                    Ok(()) => {}
                    Err(_) => {
                        self.inner.process().abort();
                        return;
                    }
                };

                self.inner.resume(Some(crate::WasmValue::I32(
                    i32::try_from(notif_size_u32).unwrap(),
                )));
            }
            LocalThreadState::OtherExtrinsicWait { mut context, .. } => {
                // TODO: the way this is handled is clearly not great; the API of this method
                // should be improved
                let decoded = redshirt_syscalls::ffi::decode_notification(&notif.0).unwrap();
                let message = decoded.actual_data.unwrap();

                assert_eq!(index, 0);
                let action = self
                    .inner
                    .process()
                    .user_data()
                    .extrinsics
                    .inject_message_response(
                        &mut context,
                        Some(message),
                        &mut MemoryAccessImpl(&mut self.inner),
                    );
                self.inner.user_data_mut().state =
                    LocalThreadState::OtherExtrinsicApplyAction { context, action };
                self.process.parent.local_run_queue.push(self.inner.tid());
            }
            _ => unreachable!(),
        }
    }

    /// Resume the thread, indicating that the notification is too large for the provided buffer.
    pub fn resume_notification_too_big(mut self, notif_size: usize) {
        debug_assert!({
            let expected = match &mut self.inner.user_data_mut().state {
                LocalThreadState::NotificationWait(wait) => wait.out_size,
                LocalThreadState::OtherExtrinsicWait { .. } => panic!(),
                _ => unreachable!(),
            };
            expected < u32::try_from(notif_size).unwrap()
        });

        self.inner.user_data_mut().state = LocalThreadState::ReadyToRun;
        self.inner.resume(Some(crate::WasmValue::I32(
            i32::try_from(notif_size).unwrap(),
        )));
    }

    /// Resume the thread, indicating that no notification is available.
    ///
    /// # Panic
    ///
    /// - Panics if [`block`](ThreadWaitNotif::block) would
    /// return `true`.
    ///
    pub fn resume_no_notification(mut self) {
        match self.inner.user_data().state {
            LocalThreadState::NotificationWait(ref wait) => assert!(!wait.block),
            LocalThreadState::OtherExtrinsicWait { .. } => panic!(),
            _ => unreachable!(),
        }

        self.inner.user_data_mut().state = LocalThreadState::ReadyToRun;
        self.inner.resume(Some(crate::WasmValue::I32(0)));
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> ThreadAccessAccess<'a>
    for ThreadWaitNotif<'a, TPud, TTud, TExt>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&self) -> ThreadId {
        self.inner.tid()
    }

    fn pid(&self) -> Pid {
        self.inner.process().pid()
    }

    fn process_user_data(&self) -> &TPud {
        self.process.user_data()
    }

    fn user_data(&mut self) -> &mut TTud {
        &mut self.inner.user_data_mut().external_user_data
    }
}

impl<'a, TPud, TTud, TExt: Extrinsics> fmt::Debug for ThreadWaitNotif<'a, TPud, TTud, TExt> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
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

/// Implementation of the [`ExtrinsicsMemoryAccess`] trait for a process.
struct MemoryAccessImpl<'a, 'b, TExtr, TPud, TTud>(
    &'a mut processes::ThreadAccess<'b, TExtr, TPud, TTud>,
);

impl<'a, 'b, TExtr, TPud, TTud> ExtrinsicsMemoryAccess
    for MemoryAccessImpl<'a, 'b, TExtr, TPud, TTud>
{
    fn read_memory(&self, range: Range<u32>) -> Result<Vec<u8>, ExtrinsicsMemoryAccessErr> {
        self.0
            .read_memory(range.start, range.end.checked_sub(range.start).unwrap())
            .map_err(|processes::OutOfBoundsError| ExtrinsicsMemoryAccessErr::OutOfRange)
    }

    fn write_memory(&mut self, offset: u32, data: &[u8]) -> Result<(), ExtrinsicsMemoryAccessErr> {
        self.0
            .write_memory(offset, data)
            .map_err(|processes::OutOfBoundsError| ExtrinsicsMemoryAccessErr::OutOfRange)
    }
}
