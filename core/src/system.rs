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

pub struct System<TExtEx> {
    core: Core<TExtEx>,
    futex_waits: HashMap<(Pid, u32), SmallVec<[u64; 4]>>,

    /// List of programs to load as soon as the loader interface is available.
    // TODO: add timeout for loader interface availability
    main_programs: SmallVec<[[u8; 32]; 1]>,

    /// Set of messages that we emitted of requests to load a program from the loader interface.
    /// All these messages expect a `nametbd_loader_interface::ffi::LoadResponse` as answer.
    // TODO: call shink_to_fit from time to time
    loading_programs: HashSet<u64>,
}

pub struct SystemBuilder<TExtEx> {
    core: CoreBuilder<TExtEx>,
    startup_processes: Vec<Module>,
    /// List of programs to load as soon as the loader interface is available.
    main_programs: SmallVec<[[u8; 32]; 1]>,
}

#[derive(Debug)]
pub enum SystemRunOutcome<TExtEx> {
    ProgramFinished {
        pid: Pid,
        return_value: Option<wasmi::RuntimeValue>, // TODO: force to i32?
    },
    ProgramCrashed {
        pid: Pid,
        error: wasmi::Error,
    },
    ThreadWaitExtrinsic {
        pid: Pid,
        thread_id: ThreadId,
        extrinsic: TExtEx,
        params: Vec<wasmi::RuntimeValue>,
    },
    InterfaceMessage {
        message_id: Option<u64>,
        interface: [u8; 32],
        message: Vec<u8>,
    },
    Idle,
}

// TODO: we require Clone because of stupid borrowing issues; remove
impl<TExtEx: Clone> System<TExtEx> {
    pub fn new() -> SystemBuilder<TExtEx> {
        // We handle some low-level interfaces here.
        let core = Core::new()
            .with_interface_handler(nametbd_interface_interface::ffi::INTERFACE)
            .with_interface_handler(nametbd_threads_interface::ffi::INTERFACE);

        SystemBuilder {
            core,
            startup_processes: Vec::new(),
            main_programs: SmallVec::new(),
        }
    }

    /// After `ThreadWaitExtrinsic` has been returned, you have to call this method in order to
    /// inject back the result of the extrinsic call.
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

    pub fn run(&mut self) -> SystemRunOutcome<TExtEx> {
        // TODO: remove loop?
        loop {
            match self.core.run() {
                CoreRunOutcome::ProgramFinished {
                    process,
                    outcome: Ok(return_value),
                    ..
                } => {
                    return SystemRunOutcome::ProgramFinished {
                        pid: process,
                        return_value,
                    }
                }
                CoreRunOutcome::ProgramFinished { process, outcome: Err(error), .. } => {
                    return SystemRunOutcome::ProgramCrashed { pid: process, error: error.into() }
                }
                CoreRunOutcome::ThreadWaitExtrinsic {
                    ref mut thread,
                    ref extrinsic,
                    ref params,
                } => {
                    let pid = thread.pid();
                    return SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id: thread.id(),
                        extrinsic: extrinsic.clone(),
                        params: params.clone(),
                    };
                }
                CoreRunOutcome::ThreadWaitUnavailableInterface { .. } => unimplemented!(),

                CoreRunOutcome::MessageResponse {
                    message_id,
                    response,
                    ..
                } => {
                    if self.loading_programs.remove(&message_id) {
                        let nametbd_loader_interface::ffi::LoadResponse { result } =
                            DecodeAll::decode_all(&response).unwrap();
                        let module = Module::from_bytes(&result.unwrap());
                        self.core.execute(&module).unwrap();
                    }
                }

                CoreRunOutcome::InterfaceMessage {
                    pid,
                    message_id,
                    interface,
                    message,
                } if interface == nametbd_threads_interface::ffi::INTERFACE => {
                    let msg: nametbd_threads_interface::ffi::ThreadsMessage =
                        DecodeAll::decode_all(&message).unwrap();
                    match msg {
                        nametbd_threads_interface::ffi::ThreadsMessage::New(new_thread) => {
                            assert!(message_id.is_none());
                            self.core.process_by_id(pid).unwrap().start_thread(
                                new_thread.fn_ptr,
                                vec![wasmi::RuntimeValue::I32(new_thread.user_data as i32)],
                            );
                        }
                        nametbd_threads_interface::ffi::ThreadsMessage::FutexWake(mut wake) => {
                            assert!(message_id.is_none());
                            if let Some(list) = self.futex_waits.get_mut(&(pid, wake.addr)) {
                                while wake.nwake > 0 && !list.is_empty() {
                                    wake.nwake -= 1;
                                    let message_id = list.remove(0);
                                    self.core.answer_message(message_id, &[]);
                                }

                                if list.is_empty() {
                                    self.futex_waits.remove(&(pid, wake.addr));
                                }
                            }
                            // TODO: implement
                        }
                        nametbd_threads_interface::ffi::ThreadsMessage::FutexWait(wait) => {
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
                } if interface == nametbd_interface_interface::ffi::INTERFACE => {
                    let msg: nametbd_interface_interface::ffi::InterfaceMessage =
                        DecodeAll::decode_all(&message).unwrap();
                    println!("interface message: {:?}", msg);
                    match msg {
                        nametbd_interface_interface::ffi::InterfaceMessage::Register(
                            interface_hash,
                        ) => {
                            if interface_hash == nametbd_loader_interface::ffi::INTERFACE {
                                for hash in self.main_programs.drain() {
                                    let msg =
                                        nametbd_loader_interface::ffi::LoaderMessage::Load(hash);
                                    let id = self
                                        .core
                                        .emit_interface_message_answer(
                                            nametbd_loader_interface::ffi::INTERFACE,
                                            msg,
                                        )
                                        .unwrap();
                                    self.loading_programs.insert(id);
                                }
                            }

                            self.core
                                .set_interface_handler(interface_hash, pid)
                                .unwrap();
                            let response =
                                nametbd_interface_interface::ffi::InterfaceRegisterResponse {
                                    result: Ok(()),
                                };
                            self.core
                                .answer_message(message_id.unwrap(), &response.encode());
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

    // TODO: better API
    pub fn answer_message(&mut self, message_id: u64, response: &[u8]) {
        //println!("answered event {:?}", message_id);
        self.core.answer_message(message_id, response)
    }
}

impl<TExtEx: Clone> SystemBuilder<TExtEx> {
    // TODO: remove Clone once possible
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

        System {
            core,
            futex_waits: Default::default(),
            loading_programs: Default::default(),
            main_programs: self.main_programs,
        }
    }
}
