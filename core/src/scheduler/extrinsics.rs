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

use crate::module::Module;
use crate::scheduler::{processes, vm};
use crate::sig;
use crate::{InterfaceHash, MessageId};

use alloc::{vec, vec::Vec};
use byteorder::{ByteOrder as _, LittleEndian};
use core::{convert::TryFrom as _, fmt, mem};
use redshirt_syscalls_interface::{EncodedMessage, Pid, ThreadId};

/// Wrapper around [`ProcessesCollection`](processes::ProcessesCollection), but that interprets
/// the extrinsic calls and keeps track of the state in which threads are waiting.
///
/// The generic parameters `TPud` and `TTud` are "user data"s that are stored respectively per
/// process and per thread, and allows the user to put extra information associated to a process
/// or a thread.
pub struct ProcessesCollectionExtrinsics<TPud, TTud> {
    inner: processes::ProcessesCollection<Extrinsic, TPud, LocalThreadUserData<TTud>>,
}

/// Prototype for a `ProcessesCollectionExtrinsics` under construction.
pub struct ProcessesCollectionExtrinsicsBuilder {
    inner: processes::ProcessesCollectionBuilder<Extrinsic>,
}

/// Access to a process within the collection.
pub struct ProcessesCollectionExtrinsicsProc<'a, TPud, TTud> {
    inner: processes::ProcessesCollectionProc<'a, TPud, LocalThreadUserData<TTud>>,
}

/// Access to a thread within the collection.
pub enum ProcessesCollectionExtrinsicsThread<'a, TPud, TTud> {
    Regular(ProcessesCollectionExtrinsicsThreadRegular<'a, TPud, TTud>),
    EmitMessage(ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>),
    WaitMessage(ProcessesCollectionExtrinsicsThreadWaitMessage<'a, TPud, TTud>),
}

/// Access to a thread within the collection.
pub struct ProcessesCollectionExtrinsicsThreadRegular<'a, TPud, TTud> {
    inner: processes::ProcessesCollectionThread<'a, TPud, LocalThreadUserData<TTud>>,
}

/// Access to a thread within the collection.
pub struct ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud> {
    inner: processes::ProcessesCollectionThread<'a, TPud, LocalThreadUserData<TTud>>,
}

/// Access to a thread within the collection.
pub struct ProcessesCollectionExtrinsicsThreadWaitMessage<'a, TPud, TTud> {
    inner: processes::ProcessesCollectionThread<'a, TPud, LocalThreadUserData<TTud>>,
}

pub trait ProcessesCollectionExtrinsicsThreadAccess<'a> {
    type ProcessUserData;
    type ThreadUserData;

    /// Returns the id of the thread. Allows later retrieval by calling
    /// [`thread_by_id`](ProcessesCollectionExtrinsics::thread_by_id).
    ///
    /// [`ThreadId`]s are unique within a [`ProcessesCollectionExtrinsics`], independently from the
    /// process.
    fn tid(&mut self) -> ThreadId;

    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollectionExtrinsics::process_by_id).
    fn pid(&self) -> Pid;

    /// Returns the following thread within the next process, or `None` if this is the last thread.
    ///
    /// Threads are ordered arbitrarily. In particular, they are **not** ordered by [`ThreadId`].
    fn next_thread(
        self,
    ) -> Option<ProcessesCollectionExtrinsicsThread<'a, Self::ProcessUserData, Self::ThreadUserData>>;

    /// Returns the user data that is associated to the process.
    fn process_user_data(&mut self) -> &mut Self::ProcessUserData;

    /// Returns the user data that is associated to the thread.
    fn user_data(&mut self) -> &mut Self::ThreadUserData;
}

/// How a process is waiting for messages.
#[derive(Debug, PartialEq, Eq)]
struct MessageWait {
    /// Identifiers of the messages we are waiting upon. Copy of what is in the process's memory.
    msg_ids: Vec<MessageId>,
    /// Offset within the memory of the process where the list of messages to wait upon is
    /// located. This is necessary as we have to zero.
    msg_ids_ptr: u32,
    /// Offset within the memory of the process where to write the received message.
    out_pointer: u32,
    /// Size of the memory of the process dedicated to receiving the message.
    out_size: u32,
    /// Whether to block the thread if no message is available.
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
/// of the thread.
#[derive(Debug)]
struct LocalThreadUserData<TTud> {
    /// State of a thread.
    state: LocalThreadState,
    /// User data decided by the user.
    external_user_data: TTud,
}

/// State of a thread. Stored within the [`processes::ProcessesCollection`].
#[derive(Debug)]
enum LocalThreadState {
    /// Thread is ready to run, running, or has just called an extrinsic and the call is being
    /// processed.
    ReadyToRun,

    /// The thread is sleeping and waiting for a message to come.
    MessageWait(MessageWait),

    /// The thread called `emit_message` and wants to emit a message on an interface.
    EmitMessage(EmitMessage),
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
        /// Process whose thread has finished.
        process: ProcessesCollectionExtrinsicsProc<'a, TPud, TTud>,

        /// User data of the thread.
        user_data: TTud,

        /// Value returned by the function that was executed.
        value: Option<wasmi::RuntimeValue>,
    },

    /// A thread in a process wants to emit a message.
    ThreadEmitMessage(ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>),

    /// A thread in a process is waiting for an incoming message.
    ThreadWaitMessage(ProcessesCollectionExtrinsicsThreadWaitMessage<'a, TPud, TTud>),

    /// A thread in a process wants to answer a message.
    ThreadEmitAnswer {
        /// Thread that wants to emit an answer.
        thread: ProcessesCollectionExtrinsicsThreadRegular<'a, TPud, TTud>,

        /// Message to answer.
        message_id: MessageId,

        /// The answer it self.
        response: EncodedMessage,
    },

    /// A thread in a process wants to notify that a message is erroneous.
    ThreadEmitMessageError {
        /// Thread that wants to emit a message error.
        thread: ProcessesCollectionExtrinsicsThreadRegular<'a, TPud, TTud>,

        /// Message that is erroneous.
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
        &mut self,
        module: &Module,
        proc_user_data: TPud,
        main_thread_user_data: TTud,
    ) -> Result<ProcessesCollectionExtrinsicsProc<TPud, TTud>, vm::NewErr> {
        let main_thread_user_data = LocalThreadUserData {
            state: LocalThreadState::ReadyToRun,
            external_user_data: main_thread_user_data,
        };
        let process = self
            .inner
            .execute(module, proc_user_data, main_thread_user_data)?;
        Ok(ProcessesCollectionExtrinsicsProc { inner: process })
    }

    /// Runs one thread amongst the collection.
    ///
    /// Which thread is run is implementation-defined and no guarantee is made.
    pub fn run(&mut self) -> RunOneOutcome<TPud, TTud> {
        match self.inner.run() {
            processes::RunOneOutcome::ProcessFinished {
                pid,
                user_data,
                dead_threads,
                outcome,
            } => RunOneOutcome::ProcessFinished {
                pid,
                user_data,
                dead_threads: dead_threads
                    .into_iter()
                    .map(|s| s.external_user_data)
                    .collect(), // TODO: meh for allocation
                outcome,
            },
            processes::RunOneOutcome::ThreadFinished {
                process,
                user_data,
                value,
            } => {
                debug_assert!(user_data.state.is_ready_to_run());
                RunOneOutcome::ThreadFinished {
                    process: ProcessesCollectionExtrinsicsProc { inner: process },
                    user_data: user_data.external_user_data,
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
                let next_msg = match parse_extrinsic_next_message(&mut thread, params) {
                    Ok(m) => m,
                    Err(_) => panic!(), // TODO:
                };
                thread.user_data().state = LocalThreadState::MessageWait(next_msg);
                RunOneOutcome::ThreadWaitMessage(ProcessesCollectionExtrinsicsThreadWaitMessage {
                    inner: thread,
                })
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
                RunOneOutcome::ThreadEmitMessage(ProcessesCollectionExtrinsicsThreadEmitMessage {
                    inner: thread,
                })
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::EmitAnswer,
                params,
            } => {
                debug_assert!(thread.user_data().state.is_ready_to_run());
                let emit_resp = match parse_extrinsic_emit_answer(&mut thread, params) {
                    Ok(m) => m,
                    Err(_) => panic!(), // TODO:
                };
                thread.resume(None);
                RunOneOutcome::ThreadEmitAnswer {
                    thread: ProcessesCollectionExtrinsicsThreadRegular { inner: thread },
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
                let emit_msg_error = match parse_extrinsic_emit_message_error(&mut thread, params) {
                    Ok(m) => m,
                    Err(_) => panic!(), // TODO:
                };
                thread.resume(None);
                RunOneOutcome::ThreadEmitMessageError {
                    thread: ProcessesCollectionExtrinsicsThreadRegular { inner: thread },
                    message_id: emit_msg_error,
                }
            }

            processes::RunOneOutcome::Interrupted {
                mut thread,
                id: Extrinsic::CancelMessage,
                params,
            } => unimplemented!(),
        }
    }

    /// Returns an iterator to all the processes that exist in the collection.
    pub fn pids<'a>(&'a self) -> impl ExactSizeIterator<Item = Pid> + 'a {
        self.inner.pids()
    }

    /// Returns a process by its [`Pid`], if it exists.
    pub fn process_by_id(
        &mut self,
        pid: Pid,
    ) -> Option<ProcessesCollectionExtrinsicsProc<TPud, TTud>> {
        let inner = self.inner.process_by_id(pid)?;
        Some(ProcessesCollectionExtrinsicsProc { inner })
    }

    /// Returns a thread by its [`ThreadId`], if it exists.
    pub fn thread_by_id(
        &mut self,
        id: ThreadId,
    ) -> Option<ProcessesCollectionExtrinsicsThread<TPud, TTud>> {
        let inner = self.inner.thread_by_id(id)?;
        Some(ProcessesCollectionExtrinsicsThread::from_inner(inner))
    }
}

impl Default for ProcessesCollectionExtrinsicsBuilder {
    fn default() -> ProcessesCollectionExtrinsicsBuilder {
        let mut inner = processes::ProcessesCollectionBuilder::default()
            .with_extrinsic(
                "redshirt",
                "next_message",
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
    pub fn build<TPud, TTud>(mut self) -> ProcessesCollectionExtrinsics<TPud, TTud> {
        ProcessesCollectionExtrinsics {
            inner: self.inner.build(),
        }
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsProc<'a, TPud, TTud> {
    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollection::process_by_id).
    pub fn pid(&self) -> Pid {
        self.inner.pid()
    }

    /// Returns the user data that is associated to the process.
    pub fn user_data(&mut self) -> &mut TPud {
        self.inner.user_data()
    }

    /// Adds a new thread to the process, starting the function with the given index and passing
    /// the given parameters.
    ///
    /// > **Note**: The "function ID" is the index of the function in the WASM module. WASM
    /// >           doesn't have function pointers. Instead, all the functions are part of a single
    /// >           global array of functions.
    // TODO: don't expose wasmi::RuntimeValue in the API
    pub fn start_thread(
        mut self,
        fn_index: u32,
        params: Vec<wasmi::RuntimeValue>,
        user_data: TTud,
    ) -> Result<ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>, vm::StartErr> {
        let thread = self.inner.start_thread(
            fn_index,
            params,
            LocalThreadUserData {
                state: LocalThreadState::ReadyToRun,
                external_user_data: user_data,
            },
        )?;

        Ok(From::from(ProcessesCollectionExtrinsicsThreadRegular {
            inner: thread,
        }))
    }

    /// Returns an object representing the main thread of this process.
    ///
    /// The "main thread" of a process is created automatically when you call
    /// [`ProcessesCollection::execute`]. If it stops, the entire process stops.
    pub fn main_thread(self) -> ProcessesCollectionExtrinsicsThread<'a, TPud, TTud> {
        ProcessesCollectionExtrinsicsThread::from_inner(self.inner.main_thread())
    }

    /// Aborts the process and returns the associated user data.
    pub fn abort(self) -> (TPud, Vec<(ThreadId, TTud)>) {
        //self.inner.abort()
        unimplemented!()
    }
}

impl<'a, TPud, TTud> fmt::Debug for ProcessesCollectionExtrinsicsProc<'a, TPud, TTud>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: threads user data
        f.debug_struct("ProcessesCollectionExtrinsicsProc")
            .field("pid", &self.pid())
            //.field("user_data", self.user_data())     // TODO: requires &mut self :-/
            .finish()
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThread<'a, TPud, TTud> {
    fn from_inner(
        inner: processes::ProcessesCollectionThread<'a, TPud, LocalThreadUserData<TTud>>,
    ) -> Self {
        enum Ty {
            Regular,
            Emit,
            Wait,
        }

        let ty = match inner.user_data().state {
            LocalThreadState::ReadyToRun => Ty::Regular,
            LocalThreadState::EmitMessage(_) => Ty::Emit,
            LocalThreadState::MessageWait(_) => Ty::Wait,
        };

        match ty {
            Ty::Regular => From::from(ProcessesCollectionExtrinsicsThreadRegular { inner }),
            Ty::Emit => From::from(ProcessesCollectionExtrinsicsThreadEmitMessage { inner }),
            Ty::Wait => From::from(ProcessesCollectionExtrinsicsThreadWaitMessage { inner }),
        }
    }
}

impl<'a, TPud, TTud> From<ProcessesCollectionExtrinsicsThreadRegular<'a, TPud, TTud>>
    for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>
{
    fn from(thread: ProcessesCollectionExtrinsicsThreadRegular<'a, TPud, TTud>) -> Self {
        ProcessesCollectionExtrinsicsThread::Regular(thread)
    }
}

impl<'a, TPud, TTud> From<ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>>
    for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>
{
    fn from(thread: ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>) -> Self {
        ProcessesCollectionExtrinsicsThread::EmitMessage(thread)
    }
}

impl<'a, TPud, TTud> From<ProcessesCollectionExtrinsicsThreadWaitMessage<'a, TPud, TTud>>
    for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>
{
    fn from(thread: ProcessesCollectionExtrinsicsThreadWaitMessage<'a, TPud, TTud>) -> Self {
        ProcessesCollectionExtrinsicsThread::WaitMessage(thread)
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThreadAccess<'a>
    for ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&mut self) -> ThreadId {
        match self {
            ProcessesCollectionExtrinsicsThread::Regular(t) => t.tid(),
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => t.tid(),
            ProcessesCollectionExtrinsicsThread::WaitMessage(t) => t.tid(),
        }
    }

    fn pid(&self) -> Pid {
        match self {
            ProcessesCollectionExtrinsicsThread::Regular(t) => t.pid(),
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => t.pid(),
            ProcessesCollectionExtrinsicsThread::WaitMessage(t) => t.pid(),
        }
    }

    fn next_thread(mut self) -> Option<ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>> {
        match self {
            ProcessesCollectionExtrinsicsThread::Regular(t) => t.next_thread(),
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => t.next_thread(),
            ProcessesCollectionExtrinsicsThread::WaitMessage(t) => t.next_thread(),
        }
    }

    fn process_user_data(&mut self) -> &mut TPud {
        match self {
            ProcessesCollectionExtrinsicsThread::Regular(t) => t.process_user_data(),
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => t.process_user_data(),
            ProcessesCollectionExtrinsicsThread::WaitMessage(t) => t.process_user_data(),
        }
    }

    fn user_data(&mut self) -> &mut TTud {
        match self {
            ProcessesCollectionExtrinsicsThread::Regular(t) => t.user_data(),
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => t.user_data(),
            ProcessesCollectionExtrinsicsThread::WaitMessage(t) => t.user_data(),
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
            ProcessesCollectionExtrinsicsThread::Regular(t) => fmt::Debug::fmt(t, f),
            ProcessesCollectionExtrinsicsThread::EmitMessage(t) => fmt::Debug::fmt(t, f),
            ProcessesCollectionExtrinsicsThread::WaitMessage(t) => fmt::Debug::fmt(t, f),
        }
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThreadAccess<'a>
    for ProcessesCollectionExtrinsicsThreadRegular<'a, TPud, TTud>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&mut self) -> ThreadId {
        self.inner.tid()
    }

    fn pid(&self) -> Pid {
        self.inner.pid()
    }

    fn next_thread(mut self) -> Option<ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>> {
        self.inner
            .next_thread()
            .map(ProcessesCollectionExtrinsicsThread::from_inner)
    }

    fn process_user_data(&mut self) -> &mut TPud {
        self.inner.process_user_data()
    }

    fn user_data(&mut self) -> &mut TTud {
        &mut self.inner.user_data().external_user_data
    }
}

impl<'a, TPud, TTud> fmt::Debug for ProcessesCollectionExtrinsicsThreadRegular<'a, TPud, TTud>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThreadAccess<'a>
    for ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&mut self) -> ThreadId {
        self.inner.tid()
    }

    fn pid(&self) -> Pid {
        self.inner.pid()
    }

    fn next_thread(mut self) -> Option<ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>> {
        self.inner
            .next_thread()
            .map(ProcessesCollectionExtrinsicsThread::from_inner)
    }

    fn process_user_data(&mut self) -> &mut TPud {
        self.inner.process_user_data()
    }

    fn user_data(&mut self) -> &mut TTud {
        &mut self.inner.user_data().external_user_data
    }
}

impl<'a, TPud, TTud> fmt::Debug for ProcessesCollectionExtrinsicsThreadEmitMessage<'a, TPud, TTud>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThreadWaitMessage<'a, TPud, TTud> {
    /// Returns the list of message IDs that the thread is waiting on. In order.
    pub fn message_ids_iter<'b>(&'b self) -> impl Iterator<Item = MessageId> + 'b {
        if let LocalThreadState::MessageWait(ref wait) = self.inner.user_data().state {
            wait.msg_ids.iter().cloned()
        } else {
            unreachable!()
        }
    }

    /// Returns the maximum size allowed for a message.
    pub fn allowed_message_size(&self) -> usize {
        if let LocalThreadState::MessageWait(ref wait) = self.inner.user_data().state {
            usize::try_from(wait.out_size).unwrap()
        } else {
            unreachable!()
        }
    }

    /// Resume the thread, sending back a message.
    ///
    /// `index` must be the index within the list returned by [`message_ids_iter`].
    ///
    /// # Panic
    ///
    /// - Panics if the message is too large. You should make sure this is not the case before
    /// calling this function.
    /// - Panics if `index` is too large.
    ///
    pub fn resume_message(self, index: usize, message: EncodedMessage) {
        let wait = {
            match mem::replace(
                &mut self.inner.user_data().state,
                LocalThreadState::ReadyToRun,
            ) {
                LocalThreadState::MessageWait(wait) => wait,
                _ => unreachable!(),
            }
        };

        assert!(index < wait.msg_ids.len());
        let message_size_u32 = u32::try_from(message.0.len()).unwrap();
        assert!(wait.out_size >= message_size_u32);

        // Write the message in the process's memory.
        match self.inner.write_memory(wait.out_pointer, &message.0) {
            Ok(()) => {}
            Err(_) => panic!(),
        };

        // Zero the corresponding entry in the messages to wait upon.
        match self.inner.write_memory(
            wait.msg_ids_ptr + u32::try_from(index).unwrap() * 8,
            &[0; 8],
        ) {
            Ok(()) => {}
            Err(_) => panic!(),
        };

        self.inner.user_data().state = LocalThreadState::ReadyToRun;
        self.inner.resume(Some(wasmi::RuntimeValue::I32(
            i32::try_from(message_size).unwrap(),
        )));
    }

    /// Resume the thread, indicating that the message is too large for the provided buffer.
    pub fn resume_message_too_big(self, message_size: usize) {
        self.inner.user_data().state = LocalThreadState::ReadyToRun;
        self.inner.resume(Some(wasmi::RuntimeValue::I32(
            i32::try_from(message_size).unwrap(),
        )));
    }

    /// Resume the thread, indicating that no message is available.
    ///
    /// # Panic
    ///
    /// Panics if `block` was set to `true`.
    pub fn resume_no_message(self, message_size: u32) {
        if let LocalThreadState::MessageWait(ref wait) = self.inner.user_data().state {
            assert!(!wait.block);
        } else {
            unreachable!()
        }

        self.inner.user_data().state = LocalThreadState::ReadyToRun;
        self.inner.resume(Some(wasmi::RuntimeValue::I32(0)));
    }
}

impl<'a, TPud, TTud> ProcessesCollectionExtrinsicsThreadAccess<'a>
    for ProcessesCollectionExtrinsicsThreadWaitMessage<'a, TPud, TTud>
{
    type ProcessUserData = TPud;
    type ThreadUserData = TTud;

    fn tid(&mut self) -> ThreadId {
        self.inner.tid()
    }

    fn pid(&self) -> Pid {
        self.inner.pid()
    }

    fn next_thread(mut self) -> Option<ProcessesCollectionExtrinsicsThread<'a, TPud, TTud>> {
        self.inner
            .next_thread()
            .map(ProcessesCollectionExtrinsicsThread::from_inner)
    }

    fn process_user_data(&mut self) -> &mut TPud {
        self.inner.process_user_data()
    }

    fn user_data(&mut self) -> &mut TTud {
        &mut self.inner.user_data().external_user_data
    }
}

impl<'a, TPud, TTud> fmt::Debug for ProcessesCollectionExtrinsicsThreadWaitMessage<'a, TPud, TTud>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
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

/// Analyzes a call to `next_message` made by the given thread.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
fn parse_extrinsic_next_message<TPud, TTud>(
    thread: &mut processes::ProcessesCollectionThread<TPud, LocalThreadUserData<TTud>>,
    params: Vec<wasmi::RuntimeValue>,
) -> Result<MessageWait, ()> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 5);

    let msg_ids_ptr = u32::try_from(params[0].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
    // TODO: consider not copying the message ids and read memory on demand instead
    let msg_ids = {
        let len = u32::try_from(params[1].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
        if len >= 512 {
            // TODO: arbitrary limit in order to not allocate too much memory below; a bit crappy
            return Err(());
        }
        let mem = thread.read_memory(msg_ids_ptr, len * 8)?;
        let mut out = vec![MessageId::from(0u64); usize::try_from(len).map_err(|_| ())?];
        for (o, i) in out.iter_mut().zip(mem.chunks(8)) {
            let val = byteorder::LittleEndian::read_u64(i);
            *o = MessageId::from(val);
        }
        out
    };

    let out_pointer = u32::try_from(params[2].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
    let out_size = u32::try_from(params[3].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
    let block = params[4].try_into::<i32>().ok_or(())? != 0;

    Ok(MessageWait {
        msg_ids,
        msg_ids_ptr,
        out_pointer,
        out_size,
        block,
    })
}

/// Analyzes a call to `emit_message` made by the given thread.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
fn parse_extrinsic_emit_message<TPud, TTud>(
    thread: &mut processes::ProcessesCollectionThread<TPud, LocalThreadUserData<TTud>>,
    params: Vec<wasmi::RuntimeValue>,
) -> Result<EmitMessage, ()> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 6);

    let interface: InterfaceHash = {
        let addr = u32::try_from(params[0].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
        InterfaceHash::from(
            <[u8; 32]>::try_from(&thread.read_memory(addr, 32)?[..]).map_err(|_| ())?,
        )
    };

    let message = {
        let addr = u32::try_from(params[1].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
        let num_bufs = u32::try_from(params[2].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
        let mut out_msg = Vec::new();
        for buf_n in 0..num_bufs {
            let sub_buf_ptr = thread.read_memory(addr + 8 * buf_n, 4).map_err(|_| ())?;
            let sub_buf_ptr = LittleEndian::read_u32(&sub_buf_ptr);
            let sub_buf_sz = thread
                .read_memory(addr + 8 * buf_n + 4, 4)
                .map_err(|_| ())?;
            let sub_buf_sz = LittleEndian::read_u32(&sub_buf_sz);
            if out_msg.len() + usize::try_from(sub_buf_sz).map_err(|_| ())? >= 16 * 1024 * 1024 {
                // TODO: arbitrary maximum message length
                panic!("Max message length reached");
                //return Err(());
            }
            out_msg.extend_from_slice(
                &thread
                    .read_memory(sub_buf_ptr, sub_buf_sz)
                    .map_err(|_| ())?,
            );
        }
        EncodedMessage(out_msg)
    };

    let needs_answer = params[3].try_into::<i32>().ok_or(())? != 0;
    let allow_delay = params[4].try_into::<i32>().ok_or(())? != 0;
    let message_id_write = if needs_answer {
        Some(u32::try_from(params[5].try_into::<i32>().ok_or(())?).map_err(|_| ())?)
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

/// Analyzes a call to `emit_answer` made by the given thread.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
fn parse_extrinsic_emit_answer<TPud, TTud>(
    thread: &mut processes::ProcessesCollectionThread<TPud, LocalThreadUserData<TTud>>,
    params: Vec<wasmi::RuntimeValue>,
) -> Result<EmitAnswer, ()> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 3);

    let message_id = {
        let addr = u32::try_from(params[0].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
        let buf = thread.read_memory(addr, 8)?;
        MessageId::from(byteorder::LittleEndian::read_u64(&buf))
    };

    let response = {
        let addr = u32::try_from(params[1].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
        let sz = u32::try_from(params[2].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
        EncodedMessage(thread.read_memory(addr, sz)?)
    };

    Ok(EmitAnswer {
        message_id,
        response,
    })
}

/// Analyzes a call to `emit_message_error` made by the given thread.
/// Returns the message for which to notify of an error.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
fn parse_extrinsic_emit_message_error<TPud, TTud>(
    thread: &mut processes::ProcessesCollectionThread<TPud, LocalThreadUserData<TTud>>,
    params: Vec<wasmi::RuntimeValue>,
) -> Result<MessageId, ()> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 1);

    let msg_id = {
        let addr = u32::try_from(params[0].try_into::<i32>().ok_or(())?).map_err(|_| ())?;
        let buf = thread.read_memory(addr, 8)?;
        MessageId::from(byteorder::LittleEndian::read_u64(&buf))
    };

    Ok(msg_id)
}
