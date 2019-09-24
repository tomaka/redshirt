// Copyright(c) 2019 Pierre Krieger

use crate::interface::{Interface, InterfaceHash, InterfaceId};
use crate::module::Module;
use crate::signature::Signature;
use crate::sig;

use alloc::borrow::Cow;
use byteorder::{ByteOrder as _, LittleEndian};
use core::{convert::TryFrom, marker::PhantomData, ops::RangeBounds};
use hashbrown::{hash_map::Entry, HashMap, HashSet};
use parity_scale_codec::{Encode as _};

mod pid;
mod processes;
mod vm;

// TODO: move definition?
pub use self::pid::Pid;

/// Handles scheduling processes and inter-process communications.
pub struct Core<T> {
    /// List of running processes.
    processes: processes::ProcessesCollection<Process>,

    /// For each interface, its definition and which program is fulfilling it.
    interfaces: HashMap<InterfaceHash, InterfaceHandler>,

    /// List of functions that processes can call.
    /// The key of this map is an arbitrary `usize` that we pass to the WASM interpreter.
    /// This field is never modified after the `Core` is created.
    extrinsics: HashMap<usize, Extrinsic<T>>,

    /// Map used to resolve imports when starting a process.
    /// For each module and function name, stores the signature and an arbitrary usize that
    /// corresponds to the entry in `extrinsics`.
    /// This field is never modified after the `Core` is created.
    extrinsics_id_assign: HashMap<(InterfaceId, Cow<'static, str>), (usize, Signature)>,

    /// Identifier of the next event to generate.
    next_message_id: u64,

    /// List of messages that have been emitted and that are waiting for a response.
    // TODO: doc about hash safety
    // TODO: call shrink_to from time to time
    messages_to_answer: HashMap<u64, Pid>,
}

/// Which way an interface is handled.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InterfaceHandler {
    Process(Pid),
    External,
}

/// Possible function available to processes.
/// The [`External`](Extrinsic::External) variant corresponds to functions that the user of the
/// [`Core`] registers. The rest are handled by the [`Core`] itself.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Extrinsic<T> {
    NextMessage,
    EmitMessage,
    EmitAnswer,
    RegisterInterface,
    External(T),
}

/// Prototype for a `Core` under construction.
pub struct CoreBuilder<T> {
    /// See the corresponding field in `Core`.
    interfaces: HashMap<InterfaceHash, InterfaceHandler>,
    /// See the corresponding field in `Core`.
    extrinsics: HashMap<usize, Extrinsic<T>>,
    /// See the corresponding field in `Core`.
    extrinsics_id_assign: HashMap<(InterfaceId, Cow<'static, str>), (usize, Signature)>,
}

/// Outcome of calling [`run`](Core::run).
// TODO: #[derive(Debug)]
pub enum CoreRunOutcome<'a, T> {
    ProgramFinished {
        process: CoreProcess<'a, T>,
        return_value: Option<wasmi::RuntimeValue>, // TODO: force to i32?
    },
    ProgramCrashed {
        pid: Pid,
        error: wasmi::Error,
    },
    ProgramWaitExtrinsic {
        process: CoreProcess<'a, T>,
        extrinsic: &'a T,
        params: Vec<wasmi::RuntimeValue>,
    },
    InterfaceMessage {
        event_id: Option<u64>,
        interface: InterfaceHash,
        message: Vec<u8>,
    },
    /// Nothing to do. No process is ready to run.
    Idle,
}

/// Because of lifetime issues, this is the same as `CoreRunOutcome` but that holds `Pid`s instead
/// of `CoreProcess`es.
enum CoreRunOutcomeInner {
    ProgramFinished {
        process: Pid,
        return_value: Option<wasmi::RuntimeValue>, // TODO: force to i32?
    },
    ProgramCrashed {
        pid: Pid,
        error: wasmi::Error,
    },
    ProgramWaitExtrinsic {
        process: Pid,
        extrinsic: usize,
        params: Vec<wasmi::RuntimeValue>,
    },
    InterfaceMessage {
        event_id: Option<u64>,
        interface: InterfaceHash,
        message: Vec<u8>,
    },
    Idle,
}

/// Additional information about a process.
struct Process {
    /// Data available for retrieval by the process.
    // TODO: shrink_to_fit
    // TODO: VecDeque?
    messages_queue: Vec<syscalls::ffi::Message>,

    /// If `Some`, the process is sleeping and waiting for a message to come.
    message_wait: Option<MessageWait>,

    /// List of messages that have been received and that need an answer. Contains the PID of the
    /// emitter of the message.
    // TODO: messages uniquely belong to a process, therefore the map should be global
    messages_to_answer: Vec<(u64, Pid)>,
}

#[derive(Debug, Clone)]
struct MessageWait {
    /// Identifiers of the messages we are waiting upon.
    msg_ids: Vec<u64>,
    /// Offset within the memory of the process where to write the received message.
    out_pointer: u32,
    out_size: u32,
}

/// Access to a process within the core.
pub struct CoreProcess<'a, T> {
    /// Access to the process within the inner collection.
    process: processes::ProcessesCollectionProc<'a, Process>,
    /// Marker to keep `T` in place.
    marker: PhantomData<T>,
}

impl<T> Core<T> {
    /// Initialies a new `Core`.
    pub fn new() -> CoreBuilder<T> {
        let builder = CoreBuilder {
            interfaces: Default::default(),
            extrinsics: Default::default(),
            extrinsics_id_assign: Default::default(),
        };

        let root_interface_id: InterfaceId = From::from([0; 32]);

        // TODO: signatures
        builder
            .with_extrinsic_inner(root_interface_id.clone(), "next_message", sig!(()), Extrinsic::NextMessage)
            .with_extrinsic_inner(root_interface_id.clone(), "emit_message", sig!(()), Extrinsic::EmitMessage)
            .with_extrinsic_inner(root_interface_id.clone(), "emit_answer", sig!(()), Extrinsic::EmitAnswer)
            .with_extrinsic_inner(root_interface_id.clone(), "register_interface", sig!(()), Extrinsic::RegisterInterface)
    }

    /// Run the core once.
    // TODO: make multithreaded
    pub fn run(&mut self) -> CoreRunOutcome<T> {
        match self.run_inner() {
            CoreRunOutcomeInner::Idle => CoreRunOutcome::Idle,
            CoreRunOutcomeInner::ProgramFinished {
                process,
                return_value,
            } => CoreRunOutcome::ProgramFinished {
                process: self.process_by_id(process).unwrap(),
                return_value,
            },
            CoreRunOutcomeInner::ProgramCrashed { pid, error } => {
                CoreRunOutcome::ProgramCrashed { pid, error }
            }
            CoreRunOutcomeInner::ProgramWaitExtrinsic {
                process,
                extrinsic,
                params,
            } => CoreRunOutcome::ProgramWaitExtrinsic {
                process: CoreProcess {
                    process: self.processes.process_by_id(process).unwrap(),
                    marker: PhantomData,
                },
                extrinsic: match self.extrinsics.get(&extrinsic).unwrap() {
                    Extrinsic::External(ref token) => token,
                    _ => panic!()
                },
                params,
            },
            CoreRunOutcomeInner::InterfaceMessage { event_id, interface, message } => {
                CoreRunOutcome::InterfaceMessage { event_id, interface, message }
            },
        }
    }

    /// Because of lifetime issues, we return an enum that holds `Pid`s instead of `CoreProcess`es.
    /// Then `run` does the conversion in order to have a good API.
    // TODO: make multithreaded
    fn run_inner(&mut self) -> CoreRunOutcomeInner {
        match self.processes.run() {
            processes::RunOneOutcome::Finished { mut process, value } => {
                // TODO: must clean up all the interfaces stuff
                return CoreRunOutcomeInner::ProgramFinished {
                    process: process.pid(),
                    return_value: value,
                };
            }
            processes::RunOneOutcome::Interrupted {
                mut process,
                id,
                params,
            } => {
                // TODO: check params against signature with a debug_assert
                match self.extrinsics.get(&id).unwrap() {
                    Extrinsic::External(token) => {
                        return CoreRunOutcomeInner::ProgramWaitExtrinsic {
                            process: process.pid(),
                            extrinsic: id,
                            params,
                        };
                    }
                    Extrinsic::RegisterInterface => {
                        // TODO: lots of unwraps here
                        assert_eq!(params.len(), 1);
                        let hash = {
                            let addr = params[0].try_into::<i32>().unwrap() as usize;
                            process.read_memory(addr..addr + 32).unwrap()
                        };
                        assert_eq!(hash.len(), 32);
                        match self.interfaces.entry(TryFrom::try_from(&hash[..]).unwrap()) {
                            Entry::Occupied(_) => panic!(),
                            Entry::Vacant(e) => e.insert(InterfaceHandler::Process(process.pid())),
                        };
                        process.resume(Some(wasmi::RuntimeValue::I32(0)));
                    }
                    Extrinsic::NextMessage => {
                        // TODO: lots of unwraps here
                        assert_eq!(params.len(), 5);
                        let msg_ids = {
                            let addr = params[0].try_into::<i32>().unwrap() as usize;
                            let len = params[1].try_into::<i32>().unwrap() as usize;
                            let mem = process.read_memory(addr..addr + len * 8).unwrap();
                            let mut out = vec![0u64; len];
                            byteorder::LittleEndian::read_u64_into(&mem, &mut out);
                            out
                        };
                        let out_pointer = params[2].try_into::<i32>().unwrap() as u32;
                        let out_size = params[3].try_into::<i32>().unwrap() as u32;
                        let block = params[4].try_into::<i32>().unwrap() != 0;
                        assert!(block);     // not blocking not supported
                        assert!(process.user_data().message_wait.is_none());
                        process.user_data().message_wait = Some(MessageWait {
                            msg_ids,
                            out_pointer,
                            out_size,
                        });
                        try_resume_message_wait(&mut process);
                    }
                    Extrinsic::EmitMessage => {
                        // TODO: lots of unwraps here
                        assert_eq!(params.len(), 5);
                        let interface: InterfaceHash = {
                            let addr = params[0].try_into::<i32>().unwrap() as usize;
                            TryFrom::try_from(&process.read_memory(addr..addr + 32).unwrap()[..]).unwrap()
                        };
                        let message = {
                            let addr = params[1].try_into::<i32>().unwrap() as usize;
                            let sz = params[2].try_into::<i32>().unwrap() as usize;
                            process.read_memory(addr..addr + sz).unwrap()
                        };
                        let needs_answer = params[3].try_into::<i32>().unwrap() != 0;
                        let event_id = if needs_answer {
                            let event_id_write = params[4].try_into::<i32>().unwrap() as u32;
                            let new_message_id = self.next_message_id;
                            self.messages_to_answer.insert(new_message_id, process.pid());
                            self.next_message_id += 1;
                            let mut buf = [0; 8];
                            LittleEndian::write_u64(&mut buf, new_message_id);
                            process.write_memory(event_id_write, &buf).unwrap();
                            // TODO: process.user_data().;
                            Some(new_message_id)
                        } else {
                            None
                        };
                        println!("proc emitting message {:?} needs_answer={:?}", message, needs_answer);
                        match self.interfaces.get(&interface).unwrap() {
                            InterfaceHandler::Process(_) => unimplemented!(),
                            InterfaceHandler::External => {
                                process.resume(Some(wasmi::RuntimeValue::I32(0)));
                                return CoreRunOutcomeInner::InterfaceMessage {
                                    event_id,
                                    interface,
                                    message,
                                };
                            }
                        }
                    }
                    Extrinsic::EmitAnswer => {
                        unimplemented!()
                    }
                }
            }
            processes::RunOneOutcome::Errored {
                pid,
                user_data,
                error,
            } => {
                // TODO: must clean up all the interfaces stuff
                println!("oops, actual error! {:?}", error);
                // TODO: remove program from list and return `ProgramCrashed` event
            }
            processes::RunOneOutcome::Idle => {}
        }

        CoreRunOutcomeInner::Idle
    }

    /// Returns an object granting access to a process, if it exists.
    pub fn process_by_id(&mut self, pid: Pid) -> Option<CoreProcess<T>> {
        let p = self.processes.process_by_id(pid)?;
        Some(CoreProcess {
            process: p,
            marker: PhantomData,
        })
    }

    // TODO: better API
    pub fn answer_event(&mut self, event_id: u64, response: &[u8]) {
        let actual_message = syscalls::ffi::Message::Response(syscalls::ffi::ResponseMessage {
            message_id: event_id,
            actual_data: response.to_vec(),
        });

        if let Some(emitter_pid) = self.messages_to_answer.remove(&event_id) {
            let mut process = self.processes.process_by_id(emitter_pid).unwrap();
            let queue_was_empty = process.user_data().messages_queue.is_empty();
            process.user_data().messages_queue.push(actual_message);
            if queue_was_empty {
                try_resume_message_wait(&mut process);
            }

        } else {
            // TODO: what to do here?
            panic!("no process found with that event")
        }
    }

    /// Start executing the module passed as parameter.
    ///
    /// Each import of the [`Module`](crate::module::Module) is resolved.
    pub fn execute(&mut self, module: &Module) -> Result<CoreProcess<T>, vm::NewErr> {
        let metadata = Process {
            messages_queue: Vec::new(),
            messages_to_answer: Vec::new(),
            message_wait: None,
        };

        let extrinsics_id_assign = &mut self.extrinsics_id_assign;

        let process =
            self.processes
                .execute(module, metadata, move |interface, function, signature| {
                    if let Some((index, signature)) =
                        extrinsics_id_assign.get(&(interface.clone(), function.into()))
                    {
                        // TODO: check signature validity
                        return Ok(*index);
                    }

                    Err(())
                })?;

        Ok(CoreProcess {
            process,
            marker: PhantomData,
        })
    }
}

impl<'a, T> CoreProcess<'a, T> {
    /// Returns the [`Pid`] of the process.
    pub fn pid(&self) -> Pid {
        self.process.pid()
    }

    /// After `ProgramWaitExtrinsic` has been returned, you have to call this method in order to
    /// inject back the result of the extrinsic call.
    // TODO: don't expose wasmi::RuntimeValue
    pub fn resolve_extrinsic_call(&mut self, return_value: Option<wasmi::RuntimeValue>) {
        // TODO: check if the value type is correct
        // TODO: check that we're not waiting for an event instead, in which case it's wrong to
        //       call this function
        self.process.resume(return_value);
    }

    /// Copies the given memory range of the given process into a `Vec<u8>`.
    ///
    /// Returns an error if the range is invalid.
    // TODO: should really return &mut [u8] I think
    pub fn read_memory(&mut self, range: impl RangeBounds<usize>) -> Result<Vec<u8>, ()> {
        self.process.read_memory(range)
    }

    /// Kills the process immediately.
    pub fn abort(self) {
        self.process.abort();
    }
}

impl<T> CoreBuilder<T> {
    /// Registers a function that processes can call.
    // TODO: more docs
    pub fn with_extrinsic(
        mut self,
        interface: impl Into<InterfaceId>,
        f_name: impl Into<Cow<'static, str>>,
        signature: Signature,
        token: impl Into<T>,
    ) -> Self {
        self.with_extrinsic_inner(interface, f_name, signature, Extrinsic::External(token.into()))
    }

    /// Inner implementation of `with_extrinsic`.
    fn with_extrinsic_inner(
        mut self,
        interface: impl Into<InterfaceId>,
        f_name: impl Into<Cow<'static, str>>,
        signature: Signature,
        extrinsic: Extrinsic<T>,
    ) -> Self {
        // TODO: panic if we already have it
        let interface = interface.into();
        let f_name = f_name.into();

        let index = self.extrinsics.len();
        debug_assert!(!self.extrinsics.contains_key(&index));
        self.extrinsics_id_assign.insert((interface, f_name), (index, signature));
        self.extrinsics.insert(index, extrinsic);
        self
    }

    /// Marks the interface passed as parameter as "external".
    ///
    /// Messages destined to this interface will be returned in the [`CoreRunOutcome`] instead of
    /// being handled internally.
    pub fn with_interface_handler(mut self, interface: impl Into<InterfaceHash>) -> Self {
        // TODO: check for duplicates
        self.interfaces.insert(interface.into(), InterfaceHandler::External);
        self
    }

    /// Turns the builder into a [`Core`].
    pub fn build(mut self) -> Core<T> {
        // We're not going to modify these fields ever again, so let's free some memory.
        self.extrinsics.shrink_to_fit();
        self.extrinsics_id_assign.shrink_to_fit();
        debug_assert_eq!(self.extrinsics.len(), self.extrinsics_id_assign.len());

        Core {
            processes: processes::ProcessesCollection::new(),
            interfaces: self.interfaces,
            extrinsics: self.extrinsics,
            extrinsics_id_assign: self.extrinsics_id_assign,
            next_message_id: 2,     // 0 and 1 are special message IDs
            messages_to_answer: HashMap::default(),
        }
    }
}

/// If the process passed as parameter is waiting for a message to arrive, checks the queue to see
/// if we can resume said process.
fn try_resume_message_wait(process: &mut processes::ProcessesCollectionProc<Process>) {
    if process.user_data().message_wait.is_none() {
        return;
    }

    let first_msg_id = if process.user_data().messages_queue.is_empty() {
        return;
    } else {
        match &process.user_data().messages_queue[0] {
            syscalls::ffi::Message::Interface(_) => 1,
            syscalls::ffi::Message::Response(response) => {
                debug_assert!(response.message_id >= 2);
                response.message_id
            },
        }
    };

    let msg_wait = process.user_data().message_wait.clone().unwrap();       // TODO: don't clone
    if !msg_wait.msg_ids.iter().any(|id| *id == first_msg_id) {
        return;
    }

    let first_msg_bytes = process.user_data().messages_queue[0].encode();
    if msg_wait.out_size as usize >= first_msg_bytes.len() {       // TODO: don't use as
        process.write_memory(msg_wait.out_pointer, &first_msg_bytes).unwrap();
    }

    process.user_data().message_wait = None;
    process.resume(Some(wasmi::RuntimeValue::I32(first_msg_bytes.len() as i32)));      // TODO: don't use as
}

#[cfg(test)]
mod tests {
    use super::{Core, CoreRunOutcome};
    use crate::{module::Module, signature::Signature};
    use core::iter;

    #[test]
    fn basic_module() {
        let module = Module::from_wat(
            r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                i32.const 5)
            (export "main" (func $main)))
        "#,
        )
        .unwrap();

        let mut core = Core::<!>::new().build();
        let expected_pid = core.execute(&module).unwrap().pid();

        match core.run() {
            CoreRunOutcome::ProgramFinished {
                process,
                return_value,
            } => {
                assert_eq!(process.pid(), expected_pid);
                assert_eq!(return_value, Some(wasmi::RuntimeValue::I32(5)));
            }
            _ => panic!(),
        }
    }

    #[test]
    #[ignore]       // TODO: test fails
    fn trapping_module() {
        let module = Module::from_wat(
            r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                unreachable)
            (export "main" (func $main)))
        "#,
        )
        .unwrap();

        let mut core = Core::<!>::new().build();
        let expected_pid = core.execute(&module).unwrap().pid();

        match core.run() {
            CoreRunOutcome::ProgramCrashed { pid, .. } => {
                assert_eq!(pid, expected_pid);
            }
            _ => panic!()
        }
    }

    #[test]
    fn module_wait_extrinsic() {
        let module = Module::from_wat(
            r#"(module
            (import "" "test" (func $test (result i32)))
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                call $test)
            (export "main" (func $main)))
        "#,
        )
        .unwrap();

        let mut core = Core::<u32>::new()
            .with_extrinsic(
                [0; 32],
                "test",
                &Signature::new(iter::empty(), None),
                639u32,
            )
            .build();

        let expected_pid = core.execute(&module).unwrap().pid();

        match core.run() {
            CoreRunOutcome::ProgramWaitExtrinsic {
                process,
                extrinsic,
                params,
            } => {
                assert_eq!(process.pid(), expected_pid);
                assert_eq!(*extrinsic, 639);
                assert!(params.is_empty());
            }
            _ => panic!(),
        }

        core.process_by_id(expected_pid)
            .unwrap()
            .resolve_extrinsic_call(Some(wasmi::RuntimeValue::I32(713)));

        match core.run() {
            CoreRunOutcome::ProgramFinished {
                process,
                return_value,
            } => {
                assert_eq!(process.pid(), expected_pid);
                assert_eq!(return_value, Some(wasmi::RuntimeValue::I32(713)));
            }
            _ => panic!(),
        }
    }
}
