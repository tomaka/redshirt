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

use crate::module::Module;
use crate::scheduler::{Core, CoreBuilder, CoreRunOutcome, Pid, ThreadId};
use crate::signature::Signature;
use alloc::{borrow::Cow, vec, vec::Vec};
use hashbrown::{hash_map::Entry, HashMap, HashSet};
use parity_scale_codec::{DecodeAll, Encode};
use smallvec::SmallVec;

/// Main struct that handles a system, including the scheduler, program loader,
/// inter-process communication, and so on.
///
/// Natively handles the "interface" and "threads" interfaces.  TODO: indicate hashes
pub struct System<TExtEx> {
    /// Inner system with inter-process communications.
    core: Core<TExtEx>,

    /// List of active futexes. The keys of this hashmap are process IDs and memory addresses, and
    /// the values of this hashmap are a list of "wait" messages to answer once the corresponding
    /// futex is woken up.
    ///
    /// Lists of messages must never be empty.
    ///
    /// Messages are always pushed at the back of the list. Therefore the first element is the
    /// oldest message.
    ///
    /// See the "threads" interface for documentation about what a futex is.
    futex_waits: HashMap<(Pid, u32), SmallVec<[u64; 4]>>,

    /// List of programs to load as soon as a loader interface handler is available.
    ///
    /// As soon as a handler for the "loader" interface is registered, we start loading the
    /// programs in this list. Afterwards, the list will always be empty.
    ///
    /// Because this list is only filled at initialization, emptied at once, and then never filled
    /// again, the most straight-forward container is a `Vec`.
    // TODO: add timeout for loader interface availability
    main_programs: Vec<[u8; 32]>,

    /// Set of messages that we emitted of requests to load a program from the loader interface.
    /// All these messages expect a `redshirt_loader_interface::ffi::LoadResponse` as answer.
    // TODO: call shink_to_fit from time to time
    loading_programs: HashSet<u64>,
}

/// Prototype for a [`System`].
pub struct SystemBuilder<TExtEx> {
    /// Builder for the inner core.
    core: CoreBuilder<TExtEx>,

    /// List of programs to start executing immediately after construction.
    startup_processes: Vec<Module>,

    /// Same field as [`System::main_programs`].
    main_programs: Vec<[u8; 32]>,
}

/// Outcome of running the [`System`] once.
#[derive(Debug)]
pub enum SystemRunOutcome<TExtEx> {
    /// A program has ended, either successfully or after an error.
    ProgramFinished {
        /// Identifier of the process that has stopped.
        pid: Pid,
        /// Either `Ok(())` if the main thread has ended, or the error that happened in the
        /// process.
        // TODO: change error type
        outcome: Result<(), wasmi::Error>,
    },

    /// A thread has called an extrinsic that was registered using
    /// [`SystemBuilder::with_extrinsic`].
    ThreadWaitExtrinsic {
        // TODO: return an object representing the thread
        /// Process that called the extrinsic.
        pid: Pid,
        /// Thread that called the extrinsic.
        thread_id: ThreadId,
        /// Identifier of the extrinsic. Matches what was passed to
        /// [`SystemBuilder::with_extrinsic`].
        extrinsic: TExtEx,
        /// Parameters passed to the extrinsic.
        params: Vec<wasmi::RuntimeValue>,
    },

    /// A thread has sent a message on an interface that was registered using
    /// [`SystemBuilder::with_interface_handler`].
    InterfaceMessage {
        // TODO: return an object representing the process or message
        /// If `Some`, identifier of the message to use to send the answer. If `None`, the message
        /// doesn't expect any answer.
        message_id: Option<u64>,
        /// Interface the message was emitted on. Matches what was passed to
        /// [`SystemBuilder::with_interface_handler`].
        interface: [u8; 32],
        /// The bytes of the message.
        message: Vec<u8>,
    },

    /// No thread is ready to run. Nothing to do.
    Idle,
}

// TODO: we require Clone because of stupid borrowing issues; remove
impl<TExtEx: Clone> System<TExtEx> {
    /// After [`SystemRunOutcome::ThreadWaitExtrinsic`] has been returned, call this method in
    /// order to inject back the result of the extrinsic call.
    // TODO: don't expose wasmi::RuntimeValue
    pub fn resolve_extrinsic_call(
        &mut self,
        thread: ThreadId,
        return_value: Option<wasmi::RuntimeValue>,
    ) {
        // TODO: can the user badly misuse that API?
        self.core
            .thread_by_id(thread)
            .unwrap()
            .resolve_extrinsic_call(return_value);
    }

    /// Runs the [`System`] once and returns the outcome.
    pub fn run(&mut self) -> SystemRunOutcome<TExtEx> {
        // TODO: remove loop?
        loop {
            match self.core.run() {
                CoreRunOutcome::ProgramFinished {
                    process, outcome, ..
                } => {
                    return SystemRunOutcome::ProgramFinished {
                        pid: process,
                        outcome: outcome.map(|_| ()).map_err(|err| err.into()),
                    }
                }
                CoreRunOutcome::ThreadWaitExtrinsic {
                    ref mut thread,
                    ref extrinsic,
                    ref params,
                } => {
                    let pid = thread.pid();
                    return SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id: thread.tid(),
                        extrinsic: extrinsic.clone(),
                        params: params.clone(),
                    };
                }
                CoreRunOutcome::ThreadWaitUnavailableInterface { .. } => {} // TODO: lazy-loading

                CoreRunOutcome::MessageResponse {
                    message_id,
                    response,
                    ..
                } => {
                    if self.loading_programs.remove(&message_id) {
                        let redshirt_loader_interface::ffi::LoadResponse { result } =
                            DecodeAll::decode_all(&response.unwrap()).unwrap();
                        let module = Module::from_bytes(&result.unwrap()).unwrap();
                        self.core.execute(&module).unwrap();
                    }
                }

                CoreRunOutcome::InterfaceMessage {
                    pid,
                    message_id,
                    interface,
                    message,
                } if interface == redshirt_threads_interface::ffi::INTERFACE => {
                    let msg: redshirt_threads_interface::ffi::ThreadsMessage =
                        DecodeAll::decode_all(&message).unwrap();
                    match msg {
                        redshirt_threads_interface::ffi::ThreadsMessage::New(new_thread) => {
                            assert!(message_id.is_none());
                            self.core.process_by_id(pid).unwrap().start_thread(
                                new_thread.fn_ptr,
                                vec![wasmi::RuntimeValue::I32(new_thread.user_data as i32)],
                            );
                        }
                        redshirt_threads_interface::ffi::ThreadsMessage::FutexWake(mut wake) => {
                            assert!(message_id.is_none());
                            if let Some(list) = self.futex_waits.get_mut(&(pid, wake.addr)) {
                                while wake.nwake > 0 && !list.is_empty() {
                                    wake.nwake -= 1;
                                    let message_id = list.remove(0);
                                    self.core.answer_message(message_id, Ok(&[]));
                                }

                                if list.is_empty() {
                                    self.futex_waits.remove(&(pid, wake.addr));
                                }
                            }
                            // TODO: implement
                        }
                        redshirt_threads_interface::ffi::ThreadsMessage::FutexWait(wait) => {
                            let message_id = message_id.unwrap();
                            // TODO: val_cmp
                            match self.futex_waits.entry((pid, wait.addr)) {
                                Entry::Occupied(mut e) => e.get_mut().push(message_id),
                                Entry::Vacant(e) => {
                                    e.insert({
                                        let mut sv = SmallVec::new();
                                        sv.push(message_id);
                                        sv
                                    });
                                }
                            }
                        }
                    }
                }

                CoreRunOutcome::InterfaceMessage {
                    pid,
                    message_id,
                    interface,
                    message,
                } if interface == redshirt_interface_interface::ffi::INTERFACE => {
                    let msg: redshirt_interface_interface::ffi::InterfaceMessage =
                        DecodeAll::decode_all(&message).unwrap();
                    match msg {
                        redshirt_interface_interface::ffi::InterfaceMessage::Register(
                            interface_hash,
                        ) => {
                            self.core
                                .set_interface_handler(interface_hash, pid)
                                .unwrap();
                            let response =
                                redshirt_interface_interface::ffi::InterfaceRegisterResponse {
                                    result: Ok(()),
                                };
                            self.core
                                .answer_message(message_id.unwrap(), Ok(&response.encode()));

                            if interface_hash == redshirt_loader_interface::ffi::INTERFACE {
                                for hash in self.main_programs.drain(..) {
                                    let msg =
                                        redshirt_loader_interface::ffi::LoaderMessage::Load(hash);
                                    let id = self
                                        .core
                                        .emit_interface_message_answer(
                                            redshirt_loader_interface::ffi::INTERFACE,
                                            msg,
                                        )
                                        .unwrap();
                                    self.loading_programs.insert(id);
                                }
                            }
                        }
                    }
                }

                CoreRunOutcome::InterfaceMessage {
                    pid,
                    message_id,
                    interface,
                    message,
                } => {
                    return SystemRunOutcome::InterfaceMessage {
                        message_id,
                        interface,
                        message,
                    };
                }

                CoreRunOutcome::Idle => return SystemRunOutcome::Idle,
            }
        }
    }

    /// Copies the given memory range of the given process into a `Vec<u8>`.
    ///
    /// Returns an error if the range is invalid.
    pub fn read_memory(&mut self, pid: Pid, offset: u32, size: u32) -> Result<Vec<u8>, ()> {
        self.core
            .process_by_id(pid)
            .ok_or(())?
            .read_memory(offset, size)
    }

    pub fn write_memory(&mut self, pid: Pid, offset: u32, data: &[u8]) -> Result<(), ()> {
        self.core
            .process_by_id(pid)
            .ok_or(())?
            .write_memory(offset, data)
    }

    /// After [`SystemRunOutcome::InterfaceMessage`] has been returned, call this method in order
    /// to send back an answer to the message.
    // TODO: better API
    pub fn answer_message(&mut self, message_id: u64, response: Result<&[u8], ()>) {
        //println!("answered event {:?}", message_id);
        self.core.answer_message(message_id, response)
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
        self.core
            .emit_interface_message_no_answer(interface, message)
    }
}

impl<TExtEx: Clone> SystemBuilder<TExtEx> {
    // TODO: remove Clone if possible
    /// Starts a new builder.
    pub fn new() -> SystemBuilder<TExtEx> {
        // We handle some low-level interfaces here.
        let core = Core::new()
            .with_interface_handler(redshirt_interface_interface::ffi::INTERFACE)
            .with_interface_handler(redshirt_threads_interface::ffi::INTERFACE);

        SystemBuilder {
            core,
            startup_processes: Vec::new(),
            main_programs: Vec::new(),
        }
    }

    /// Registers an extrinsic function as available.
    ///
    /// If a program calls this extrinsic, a [`SystemRunOutcome::ThreadWaitExtrinsic`] event will
    /// be generated for the user to handle.
    ///
    /// # Panic
    ///
    /// Panics if the extrinsic has already been registered, or if the extrinsic conflicts with
    /// one of the extrinsics natively handled by the [`System`].
    pub fn with_extrinsic(
        mut self,
        interface: impl Into<Cow<'static, str>>,
        f_name: impl Into<Cow<'static, str>>,
        signature: Signature,
        token: TExtEx,
    ) -> Self {
        self.core = self
            .core
            .with_extrinsic(interface, f_name, signature, token);
        self
    }

    /// Registers an interface as available.
    ///
    /// If a program sends a message to this interface, a [`SystemRunOutcome::InterfaceMessage`]
    /// event will be generated for the user to handle.
    ///
    /// # Panic
    ///
    /// Panics if the interface has already been registered, or if the interface conflicts with
    /// one of the interfaces natively handled by the [`System`].
    pub fn with_interface_handler(mut self, interface: impl Into<[u8; 32]>) -> Self {
        self.core = self.core.with_interface_handler(interface);
        self
    }

    /// Adds a process to the list of processes that the [`System`] must start as part of the
    /// startup process.
    ///
    /// The startup processes are started in the order in which they are added here.
    ///
    /// By default, the list is empty. Should at least contain a process that handles the `loader`
    /// interface.
    pub fn with_startup_process(mut self, process: impl Into<Module>) -> Self {
        let process = process.into();
        self.startup_processes.push(process);
        self
    }

    /// Adds a program that the [`System`] must execute after startup. Can be called multiple times
    /// to add multiple programs.
    ///
    /// The program will be loaded through the `loader` interface. The loading starts as soon as
    /// the `loader` interface has been registered by one of the processes passed to
    /// [`with_startup_process`](SystemBuilder::with_startup_process).
    pub fn with_main_program(mut self, hash: [u8; 32]) -> Self {
        self.main_programs.push(hash);
        self
    }

    /// Builds the [`System`].
    pub fn build(mut self) -> System<TExtEx> {
        let mut core = self.core.build();

        for program in self.startup_processes {
            core.execute(&program)
                .expect("failed to start startup program"); // TODO:
        }

        self.main_programs.shrink_to_fit();

        System {
            core,
            futex_waits: Default::default(),
            loading_programs: Default::default(),
            main_programs: self.main_programs,
        }
    }
}

impl<TExtEx: Clone> Default for SystemBuilder<TExtEx> {
    // TODO: remove Clone if possible
    fn default() -> Self {
        SystemBuilder::new()
    }
}
