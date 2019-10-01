// Copyright(c) 2019 Pierre Krieger

use crate::module::Module;
use crate::scheduler::{Core, CoreBuilder, CoreProcess, CoreRunOutcome, Pid, ThreadId};
use crate::signature::{Signature, ValueType};
use alloc::{borrow::Cow, vec::Vec, vec};
use core::{iter, ops::RangeBounds};
use hashbrown::{hash_map::Entry, HashMap};
use parity_scale_codec::{Decode, DecodeAll, Encode};
use smallvec::SmallVec;

pub struct System<TExtEx> {
    core: Core<Extrinsic<TExtEx>>,
    futex_waits: HashMap<(Pid, u32), SmallVec<[u64; 4]>>,
}

pub struct SystemBuilder<TExtEx> {
    core: CoreBuilder<Extrinsic<TExtEx>>,
    main_programs: SmallVec<[Module; 1]>,
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
        event_id: Option<u64>,
        interface: [u8; 32],
        message: Vec<u8>,
    },
    Idle,
}

#[derive(Debug)]
enum Extrinsic<TExtEx> {
    External(TExtEx),
}

// TODO: we require Clone because of stupid borrowing issues; remove
impl<TExtEx: Clone> System<TExtEx> {
    pub fn new() -> SystemBuilder<TExtEx> {
        // We handle some low-level interfaces here.
        let core = Core::new()
            .with_interface_handler(interface::ffi::INTERFACE)
            .with_interface_handler(threads::ffi::INTERFACE);

        SystemBuilder {
            core,
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
                    return_value,
                } => {
                    return SystemRunOutcome::ProgramFinished {
                        pid: process,
                        return_value,
                    }
                }
                CoreRunOutcome::ProgramCrashed { pid, error } => {
                    return SystemRunOutcome::ProgramCrashed { pid, error }
                }
                CoreRunOutcome::ThreadWaitExtrinsic {
                    ref mut thread,
                    extrinsic: &Extrinsic::External(ref external_token),
                    ref params,
                } => {
                    let pid = thread.pid();
                    return SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id: thread.id(),
                        extrinsic: external_token.clone(),
                        params: params.clone(),
                    };
                }
                CoreRunOutcome::InterfaceMessage {
                    pid,
                    event_id,
                    interface,
                    message,
                } if interface == threads::ffi::INTERFACE => {
                    let msg: threads::ffi::ThreadsMessage =
                        DecodeAll::decode_all(&message).unwrap();
                    println!("threads message: {:?}", msg);
                    match msg {
                        threads::ffi::ThreadsMessage::New(new_thread) => {
                            assert!(event_id.is_none());
                            self.core.process_by_id(pid).unwrap().start_thread(
                                new_thread.fn_ptr,
                                vec![wasmi::RuntimeValue::I32(new_thread.user_data as i32)],
                            );
                        }
                        threads::ffi::ThreadsMessage::FutexWake(mut wake) => {
                            assert!(event_id.is_none());
                            if let Some(list) = self.futex_waits.get_mut(&(pid, wake.addr)) {
                                while wake.nwake > 0 && !list.is_empty() {
                                    wake.nwake -= 1;
                                    let event_id = list.remove(0);
                                    self.core.answer_event(event_id, &[]);
                                }

                                if list.is_empty() {
                                    self.futex_waits.remove(&(pid, wake.addr));
                                }
                            }
                            // TODO: implement
                        }
                        threads::ffi::ThreadsMessage::FutexWait(wait) => {
                            let event_id = event_id.unwrap();
                            // TODO: val_cmp
                            match self.futex_waits.entry((pid, wait.addr)) {
                                Entry::Occupied(mut e) => e.get_mut().push(event_id),
                                Entry::Vacant(e) => {
                                    e.insert({
                                        let mut sv = SmallVec::new();
                                        sv.push(event_id);
                                        sv
                                    });
                                }
                            }
                        }
                    }
                }
                CoreRunOutcome::InterfaceMessage {
                    pid,
                    event_id,
                    interface,
                    message,
                } if interface == interface::ffi::INTERFACE => {
                    let msg: interface::ffi::InterfaceMessage =
                        DecodeAll::decode_all(&message).unwrap();
                    println!("interface message: {:?}", msg);
                    match msg {
                        interface::ffi::InterfaceMessage::Register(hash) => {
                            // TODO:
                            let response =
                                interface::ffi::InterfaceRegisterResponse { result: Ok(()) };
                            self.core
                                .answer_event(event_id.unwrap(), &response.encode());
                        }
                    }
                }
                CoreRunOutcome::InterfaceMessage {
                    pid,
                    event_id,
                    interface,
                    message,
                } => {
                    return SystemRunOutcome::InterfaceMessage {
                        event_id,
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
    pub fn answer_event(&mut self, event_id: u64, response: &[u8]) {
        println!("answered event {:?}", event_id);
        self.core.answer_event(event_id, response)
    }
}

impl<TExtEx> SystemBuilder<TExtEx> {
    pub fn with_extrinsic(
        mut self,
        interface: impl Into<Cow<'static, str>>,
        f_name: impl Into<Cow<'static, str>>,
        signature: Signature,
        token: TExtEx,
    ) -> Self {
        self.core =
            self.core
                .with_extrinsic(interface, f_name, signature, Extrinsic::External(token));
        self
    }

    pub fn with_interface_handler(mut self, interface: impl Into<[u8; 32]>) -> Self {
        self.core = self.core.with_interface_handler(interface);
        self
    }

    /// Adds a program that the [`System`] must execute on startup. Can be called multiple times
    /// to add multiple programs.
    pub fn with_main_program(mut self, module: Module) -> Self {
        self.main_programs.push(module);
        self
    }

    /// Builds the [`System`].
    pub fn build(mut self) -> System<TExtEx> {
        let mut core = self.core.build();

        for program in self.main_programs.drain() {
            core.execute(&program)
                .expect("failed to start main program"); // TODO:
        }

        System {
            core,
            futex_waits: Default::default(),
        }
    }
}
