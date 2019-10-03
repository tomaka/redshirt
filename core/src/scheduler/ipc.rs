// Copyright(c) 2019 Pierre Krieger

use crate::module::Module;
use crate::scheduler::{pid::Pid, processes, vm, ThreadId};
use crate::sig;
use crate::signature::Signature;

use alloc::{borrow::Cow, collections::VecDeque, vec, vec::Vec};
use byteorder::{ByteOrder as _, LittleEndian};
use core::{convert::TryFrom, marker::PhantomData};
use hashbrown::{HashMap, hash_map::Entry};
use parity_scale_codec::Encode;

/// Handles scheduling processes and inter-process communications.
pub struct Core<T> {
    /// List of running processes.
    processes: processes::ProcessesCollection<Process, Thread>,

    /// For each interface, which program is fulfilling it.
    interfaces: HashMap<[u8; 32], InterfaceHandler>,

    /// List of functions that processes can call.
    /// The key of this map is an arbitrary `usize` that we pass to the WASM interpreter.
    /// This field is never modified after the `Core` is created.
    extrinsics: HashMap<usize, Extrinsic<T>>,

    /// Map used to resolve imports when starting a process.
    /// For each module and function name, stores the signature and an arbitrary usize that
    /// corresponds to the entry in `extrinsics`.
    /// This field is never modified after the `Core` is created.
    extrinsics_id_assign: HashMap<(Cow<'static, str>, Cow<'static, str>), (usize, Signature)>,

    /// Identifier of the next event to generate.
    next_message_id: u64,

    /// List of messages that have been emitted and that are waiting for a response.
    // TODO: doc about hash safety
    // TODO: call shrink_to from time to time
    messages_to_answer: HashMap<u64, MessageEmitter>,
}

/// Which way an interface is handled.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InterfaceHandler {
    Process(Pid),
    External,
}

/// What was the source fo a message.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MessageEmitter {
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
    CancelMessage,
    External(T),
}

/// Prototype for a `Core` under construction.
pub struct CoreBuilder<T> {
    /// See the corresponding field in `Core`.
    interfaces: HashMap<[u8; 32], InterfaceHandler>,
    /// See the corresponding field in `Core`.
    extrinsics: HashMap<usize, Extrinsic<T>>,
    /// See the corresponding field in `Core`.
    extrinsics_id_assign: HashMap<(Cow<'static, str>, Cow<'static, str>), (usize, Signature)>,
}

/// Outcome of calling [`run`](Core::run).
// TODO: #[derive(Debug)]
pub enum CoreRunOutcome<'a, T> {
    ProgramFinished {
        process: Pid,
        return_value: Option<wasmi::RuntimeValue>, // TODO: force to i32?
    },
    ProgramCrashed {
        pid: Pid,
        error: wasmi::Error,
    },
    ThreadWaitExtrinsic {
        thread: CoreThread<'a, T>,
        extrinsic: &'a T,
        params: Vec<wasmi::RuntimeValue>,
    },
    InterfaceMessage {
        pid: Pid,
        message_id: Option<u64>,
        interface: [u8; 32],
        message: Vec<u8>,
    },
    MessageResponse {
        message_id: u64,
        pid: Pid,
        response: Vec<u8>,
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
    ThreadWaitExtrinsic {
        thread: ThreadId,
        extrinsic: usize,
        params: Vec<wasmi::RuntimeValue>,
    },
    InterfaceMessage {
        // TODO: `pid` is redundant with `message_id`; should just be a better API with an `Event` handle struct
        pid: Pid,
        message_id: Option<u64>,
        interface: [u8; 32],
        message: Vec<u8>,
    },
    MessageResponse {
        pid: Pid,
        message_id: u64,
        response: Vec<u8>,
    },
    LoopAgain,
    Idle,
}

/// Additional information about a process.
#[derive(Debug)]
struct Process {
    /// Messages available for retrieval by the process by calling `next_message`.
    ///
    /// Note that the [`ResponseMessage::index_in_list`](syscalls::ffi::ResponseMessage::index_in_list)
    /// and [`InterfaceMessage::index_in_list`](syscalls::ffi::InterfaceMessage::index_in_list) fields are
    /// set to a dummy value, and must be filled before actually delivering the message.
    // TODO: call shrink_to_fit from time to time
    messages_queue: VecDeque<syscalls::ffi::Message>,
}

/// Additional information about a thread.
#[derive(Debug, PartialEq, Eq)]
enum Thread {
    /// Thread is ready to run. Must be consistent with the actual state of the thread.
    ReadyToRun,

    /// The thread is sleeping and waiting for a message to come.
    ///
    /// Note that this can be set even if the `messages_queue` is not empty, in the case where
    /// the thread is waiting only on messages that aren't in the queue.
    MessageWait(MessageWait),

    /// The thread is sleeping and waiting for an extrinsic.
    ExtrinsicWait,
}

/// How a process is waiting for messages.
#[derive(Debug, Clone, PartialEq, Eq)] // TODO: remove Clone
struct MessageWait {
    /// Identifiers of the messages we are waiting upon. Duplicate of what is in the process's
    /// memory.
    msg_ids: Vec<u64>,
    /// Offset within the memory of the process where the list of messages to wait upon is
    /// located. This is necessary as we have to zero.
    msg_ids_ptr: u32,
    /// Offset within the memory of the process where to write the received message.
    out_pointer: u32,
    /// Size of the memory of the process dedicated to receiving the message.
    out_size: u32,
}

/// Access to a process within the core.
pub struct CoreProcess<'a, T> {
    /// Access to the process within the inner collection.
    process: processes::ProcessesCollectionProc<'a, Process, Thread>,
    /// Marker to keep `T` in place.
    marker: PhantomData<T>,
}

/// Access to a thread within the core.
pub struct CoreThread<'a, T> {
    /// Access to the thread within the inner collection.
    thread: processes::ProcessesCollectionThread<'a, Process, Thread>,
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

        let root_interface_id = "";

        // TODO: signatures
        builder
            .with_extrinsic_inner(
                root_interface_id.clone(),
                "next_message",
                sig!(()),
                Extrinsic::NextMessage,
            )
            .with_extrinsic_inner(
                root_interface_id.clone(),
                "emit_message",
                sig!(()),
                Extrinsic::EmitMessage,
            )
            .with_extrinsic_inner(
                root_interface_id.clone(),
                "emit_answer",
                sig!(()),
                Extrinsic::EmitAnswer,
            )
            .with_extrinsic_inner(
                root_interface_id.clone(),
                "cancel_message",
                sig!(()),
                Extrinsic::CancelMessage,
            )
    }

    /// Run the core once.
    // TODO: make multithreaded
    pub fn run(&mut self) -> CoreRunOutcome<T> {
        loop {
            break match self.run_inner() {
                CoreRunOutcomeInner::Idle => CoreRunOutcome::Idle,
                CoreRunOutcomeInner::LoopAgain => continue,
                CoreRunOutcomeInner::ProgramFinished {
                    process,
                    return_value,
                } => CoreRunOutcome::ProgramFinished {
                    process,
                    return_value,
                },
                CoreRunOutcomeInner::ProgramCrashed { pid, error } => {
                    CoreRunOutcome::ProgramCrashed { pid, error }
                }
                CoreRunOutcomeInner::ThreadWaitExtrinsic {
                    thread,
                    extrinsic,
                    params,
                } => CoreRunOutcome::ThreadWaitExtrinsic {
                    thread: CoreThread {
                        thread: self.processes.thread_by_id(thread).unwrap(),
                        marker: PhantomData,
                    },
                    extrinsic: match self.extrinsics.get(&extrinsic).unwrap() {
                        Extrinsic::External(ref token) => token,
                        _ => panic!(),
                    },
                    params,
                },
                CoreRunOutcomeInner::InterfaceMessage {
                    pid,
                    message_id,
                    interface,
                    message,
                } => CoreRunOutcome::InterfaceMessage {
                    pid,
                    message_id,
                    interface,
                    message,
                },
                CoreRunOutcomeInner::MessageResponse {
                    pid,
                    message_id,
                    response,
                } => CoreRunOutcome::MessageResponse {
                    pid,
                    message_id,
                    response,
                },
            };
        }
    }

    /// Because of lifetime issues, we return an enum that holds `Pid`s instead of `CoreProcess`es.
    /// Then `run` does the conversion in order to have a good API.
    // TODO: make multithreaded
    fn run_inner(&mut self) -> CoreRunOutcomeInner {
        match self.processes.run() {
            processes::RunOneOutcome::ProcessFinished { pid, value, .. } => {
                // TODO: must clean up all the interfaces stuff
                // TODO: also, what do we do with the pending messages and all?
                return CoreRunOutcomeInner::ProgramFinished {
                    process: pid,
                    return_value: value,
                };
            }
            processes::RunOneOutcome::ThreadFinished { .. } => { println!("thread finished"); }
            processes::RunOneOutcome::Interrupted {
                mut thread,
                id,
                params,
            } => {
                debug_assert_eq!(*thread.user_data(), Thread::ReadyToRun);

                // TODO: check params against signature with a debug_assert
                match self.extrinsics.get(&id).unwrap() {
                    Extrinsic::External(_) => {
                        *thread.user_data() = Thread::ExtrinsicWait;
                        return CoreRunOutcomeInner::ThreadWaitExtrinsic {
                            thread: thread.id(),
                            extrinsic: id,
                            params,
                        };
                    }

                    Extrinsic::NextMessage => {
                        extrinsic_next_message(&mut thread, params);
                        // TODO: only loop again if we resumed
                        return CoreRunOutcomeInner::LoopAgain;
                    }

                    Extrinsic::EmitMessage => {
                        // TODO: after this extrinsic is handled, we should maybe resume execution of the currently
                        // run function, rather than jumping to a different one
                        // TODO: lots of unwraps here
                        assert_eq!(params.len(), 5);
                        let interface: [u8; 32] = {
                            let addr = params[0].try_into::<i32>().unwrap() as u32;
                            TryFrom::try_from(&thread.read_memory(addr, 32).unwrap()[..]).unwrap()
                        };
                        let message = {
                            let addr = params[1].try_into::<i32>().unwrap() as u32;
                            let sz = params[2].try_into::<i32>().unwrap() as u32;
                            thread.read_memory(addr, sz).unwrap()
                        };
                        let needs_answer = params[3].try_into::<i32>().unwrap() != 0;
                        let emitter_pid = thread.pid();
                        let message_id = if needs_answer {
                            let message_id_write = params[4].try_into::<i32>().unwrap() as u32;
                            let new_message_id = self.next_message_id;
                            self.messages_to_answer.insert(new_message_id, MessageEmitter::Process(emitter_pid));
                            self.next_message_id += 1;
                            let mut buf = [0; 8];
                            LittleEndian::write_u64(&mut buf, new_message_id);
                            thread.write_memory(message_id_write, &buf).unwrap();
                            // TODO: thread.user_data().;
                            Some(new_message_id)
                        } else {
                            None
                        };
                        match self
                            .interfaces
                            .get(&interface)
                            .expect("Interface handler not found")
                        {
                            InterfaceHandler::Process(pid) => {
                                let message = syscalls::ffi::Message::Interface(syscalls::ffi::InterfaceMessage {
                                    interface,
                                    index_in_list: 0,
                                    message_id,
                                    emitter_pid: Some(emitter_pid.into()),
                                    actual_data: message,
                                });

                                let mut process = self.processes.process_by_id(*pid).unwrap();
                                process.user_data()
                                    .messages_queue
                                    .push_back(message);

                                let mut thread = process.main_thread();
                                loop {
                                    try_resume_message_wait(&mut thread);
                                    match thread.next_thread() {
                                        Some(t) => thread = t,
                                        None => break,
                                    };
                                }
                            },
                            InterfaceHandler::External => {
                                thread.resume(Some(wasmi::RuntimeValue::I32(0)));
                                return CoreRunOutcomeInner::InterfaceMessage {
                                    pid: thread.pid(),
                                    message_id,
                                    interface,
                                    message,
                                };
                            }
                        }
                    }

                    Extrinsic::EmitAnswer => {
                        // TODO: lots of unwraps here
                        assert_eq!(params.len(), 3);
                        let msg_id = {
                            let addr = params[0].try_into::<i32>().unwrap() as u32;
                            let buf = thread.read_memory(addr, 8).unwrap();
                            byteorder::LittleEndian::read_u64(&buf)
                        };
                        let message = {
                            let addr = params[1].try_into::<i32>().unwrap() as u32;
                            let sz = params[2].try_into::<i32>().unwrap() as u32;
                            thread.read_memory(addr, sz).unwrap()
                        };
                        let pid = thread.pid();
                        drop(thread);
                        return self.answer_message_inner(msg_id, &message, Some(pid))
                            .unwrap_or(CoreRunOutcomeInner::LoopAgain);
                    }

                    Extrinsic::CancelMessage => unimplemented!(),
                }
            }
            processes::RunOneOutcome::Errored { error, .. } => {
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

    /// Returns an object granting access to a thread, if it exists.
    pub fn thread_by_id(&mut self, thread: ThreadId) -> Option<CoreThread<T>> {
        let thread = self.processes.thread_by_id(thread)?;
        Some(CoreThread {
            thread,
            marker: PhantomData,
        })
    }

    // TODO: better API
    pub fn set_interface_handler(&mut self, interface: [u8; 32], process: Pid) -> Result<(), ()> {
        if self.processes.process_by_id(process).is_none() {
            return Err(());
        }

        match self.interfaces.entry(interface) {
            Entry::Occupied(_) => return Err(()),
            Entry::Vacant(e) => e.insert(InterfaceHandler::Process(process)),
        };

        Ok(())
    }

    /// Emits a message for the handler of the given interface.
    ///
    /// The message doesn't expect any answer.
    // TODO: better API
    pub fn emit_interface_message_no_answer(&mut self, interface: [u8; 32], message: impl Encode) -> Result<(), ()> {
        let message = syscalls::ffi::Message::Interface(syscalls::ffi::InterfaceMessage {
            interface,
            message_id: None,
            emitter_pid: None,
            index_in_list: 0,
            actual_data: message.encode(),
        });

        let pid = match self.interfaces.get(&interface).ok_or(())? {
            InterfaceHandler::Process(pid) => *pid,
            InterfaceHandler::External => return Err(()),       // TODO: explain that explicitely
        };

        let mut process = self.processes.process_by_id(pid).unwrap();
        process.user_data()
            .messages_queue
            .push_back(message);

        let mut thread = process.main_thread();
        loop {
            try_resume_message_wait(&mut thread);
            match thread.next_thread() {
                Some(t) => thread = t,
                None => break,
            };
        }

        Ok(())
    }

    /// Emits a message for the handler of the given interface.
    ///
    /// The message does expect an answer. The answer will be sent back as
    /// [`MessageResponse`](CoreRunOutcome::MessageResponse) event.
    // TODO: better API
    pub fn emit_interface_message_answer(&mut self, interface: [u8; 32], message: impl Encode) -> Result<u64, ()> {
        let message_id = {
            let id = self.next_message_id;
            self.next_message_id += 1;
            id
        };

        let message = syscalls::ffi::Message::Interface(syscalls::ffi::InterfaceMessage {
            interface,
            message_id: Some(message_id),
            emitter_pid: None,
            index_in_list: 0,
            actual_data: message.encode(),
        });

        let pid = match self.interfaces.get(&interface).ok_or(())? {
            InterfaceHandler::Process(pid) => *pid,
            InterfaceHandler::External => return Err(()),       // TODO: explain that explicitely
        };

        let mut process = self.processes.process_by_id(pid).unwrap();
        process.user_data()
            .messages_queue
            .push_back(message);

        let mut thread = process.main_thread();
        loop {
            try_resume_message_wait(&mut thread);
            match thread.next_thread() {
                Some(t) => thread = t,
                None => break,
            };
        }

        let _old_val = self.messages_to_answer.insert(message_id, MessageEmitter::External);
        debug_assert!(_old_val.is_none());

        Ok(message_id)
    }

    ///
    ///
    /// It is forbidden to answer messages created using [`emit_interface_message_answer`] or
    /// [`emit_interface_message_no_answer`]. Only messages generated by processes can be answered
    /// through this method.
    // TODO: better API
    pub fn answer_message(&mut self, message_id: u64, response: &[u8]) {
        let ret = self.answer_message_inner(message_id, response, None);
        assert!(ret.is_none());
    }

    // TODO: better API
    fn answer_message_inner(&mut self, message_id: u64, response: &[u8], answerer_pid: Option<Pid>) -> Option<CoreRunOutcomeInner> {
        let actual_message = syscalls::ffi::Message::Response(syscalls::ffi::ResponseMessage {
            message_id,
            // We a dummy value here and fill it up later when actually delivering the message.
            index_in_list: 0,
            actual_data: response.to_vec(),
        });

        match (self.messages_to_answer.remove(&message_id), answerer_pid) {
            (Some(MessageEmitter::Process(emitter_pid)), _) => {
                let mut process = self.processes.process_by_id(emitter_pid).unwrap();
                process.user_data().messages_queue.push_back(actual_message);

                let mut thread = process.main_thread();
                loop {
                    try_resume_message_wait(&mut thread);
                    match thread.next_thread() {
                        Some(t) => thread = t,
                        None => break,
                    };
                }

                None
            }
            (Some(MessageEmitter::External), Some(answerer_pid)) => {
                Some(CoreRunOutcomeInner::MessageResponse {
                    pid: answerer_pid, 
                    message_id,
                    response: response.to_vec(),
                })
            }
            (None, _) | (Some(MessageEmitter::External), None) => {
                // TODO: what to do here?
                panic!("no process found with that event")
            }
        }
    }

    /// Start executing the module passed as parameter.
    ///
    /// Each import of the [`Module`](crate::module::Module) is resolved.
    pub fn execute(&mut self, module: &Module) -> Result<CoreProcess<T>, vm::NewErr> {
        let proc_metadata = Process {
            messages_queue: VecDeque::new(),
        };

        let extrinsics_id_assign = &mut self.extrinsics_id_assign;

        let process = self.processes.execute(
            module,
            proc_metadata,
            Thread::ReadyToRun,
            move |interface, function, obtained_signature| {
                if let Some((index, expected_signature)) =
                    extrinsics_id_assign.get(&(interface.into(), function.into()))
                {
                    // TODO: check signature validity
                    return Ok(*index);
                }

                Err(())
            },
        )?;

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

    /// Adds a new thread to the process, starting the function with the given index and passing
    /// the given parameters.
    // TODO: don't expose wasmi::RuntimeValue
    pub fn start_thread(
        self,
        fn_index: u32,
        params: Vec<wasmi::RuntimeValue>,
    ) -> Result<CoreThread<'a, T>, vm::StartErr> {
        let thread = self
            .process
            .start_thread(fn_index, params, Thread::ReadyToRun)?;
        Ok(CoreThread {
            thread,
            marker: PhantomData,
        })
    }

    /// Copies the given memory range of the given process into a `Vec<u8>`.
    ///
    /// Returns an error if the range is invalid.
    pub fn read_memory(&mut self, offset: u32, size: u32) -> Result<Vec<u8>, ()> {
        self.process.read_memory(offset, size)
    }

    pub fn write_memory(&mut self, offset: u32, data: &[u8]) -> Result<(), ()> {
        self.process.write_memory(offset, data)
    }

    /// Kills the process immediately.
    pub fn abort(self) {
        self.process.abort();
    }
}

impl<'a, T> CoreThread<'a, T> {
    /// Returns the [`ThreadId`] of the thread.
    pub fn id(&mut self) -> ThreadId {
        self.thread.id()
    }

    /// Returns the [`Pid`] of the process associated to this thread.
    pub fn pid(&self) -> Pid {
        self.thread.pid()
    }

    /// After `ThreadWaitExtrinsic` has been returned, you have to call this method in order to
    /// inject back the result of the extrinsic call.
    // TODO: don't expose wasmi::RuntimeValue
    pub fn resolve_extrinsic_call(&mut self, return_value: Option<wasmi::RuntimeValue>) {
        assert_eq!(*self.thread.user_data(), Thread::ExtrinsicWait);
        *self.thread.user_data() = Thread::ReadyToRun;
        // TODO: check if the value type is correct
        self.thread.resume(return_value);
    }
}

impl<T> CoreBuilder<T> {
    /// Registers a function that processes can call.
    // TODO: more docs
    pub fn with_extrinsic(
        self,
        interface: impl Into<Cow<'static, str>>,
        f_name: impl Into<Cow<'static, str>>,
        signature: Signature,
        token: impl Into<T>,
    ) -> Self {
        self.with_extrinsic_inner(
            interface,
            f_name,
            signature,
            Extrinsic::External(token.into()),
        )
    }

    /// Inner implementation of `with_extrinsic`.
    fn with_extrinsic_inner(
        mut self,
        interface: impl Into<Cow<'static, str>>,
        f_name: impl Into<Cow<'static, str>>,
        signature: Signature,
        extrinsic: Extrinsic<T>,
    ) -> Self {
        // TODO: panic if we already have it
        let interface = interface.into();
        let f_name = f_name.into();

        let index = self.extrinsics.len();
        debug_assert!(!self.extrinsics.contains_key(&index));
        self.extrinsics_id_assign
            .insert((interface, f_name), (index, signature));
        self.extrinsics.insert(index, extrinsic);
        self
    }

    /// Marks the interface passed as parameter as "external".
    ///
    /// Messages destined to this interface will be returned in the [`CoreRunOutcome`] instead of
    /// being handled internally.
    pub fn with_interface_handler(mut self, interface: impl Into<[u8; 32]>) -> Self {
        // TODO: check for duplicates
        self.interfaces
            .insert(interface.into(), InterfaceHandler::External);
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
            next_message_id: 2, // 0 and 1 are special message IDs
            messages_to_answer: HashMap::default(),
        }
    }
}

/// Called when a process calls the `next_message` extrinsic.
///
/// Tries to resume the process by fetching a message from the queue.
fn extrinsic_next_message(
    process: &mut processes::ProcessesCollectionThread<Process, Thread>,
    params: Vec<wasmi::RuntimeValue>,
) {
    // TODO: lots of unwraps here
    assert_eq!(params.len(), 5);
    let msg_ids_ptr = params[0].try_into::<i32>().unwrap() as u32;
    let msg_ids = {
        let addr = msg_ids_ptr;
        let len = params[1].try_into::<i32>().unwrap() as u32;
        let mem = process.read_memory(addr, len * 8).unwrap();
        let mut out = vec![0u64; len as usize];
        byteorder::LittleEndian::read_u64_into(&mem, &mut out);
        out
    };

    let out_pointer = params[2].try_into::<i32>().unwrap() as u32;
    let out_size = params[3].try_into::<i32>().unwrap() as u32;
    let block = params[4].try_into::<i32>().unwrap() != 0;

    assert!(*process.user_data() == Thread::ReadyToRun);
    *process.user_data() = Thread::MessageWait(MessageWait {
        msg_ids,
        msg_ids_ptr,
        out_pointer,
        out_size,
    });

    try_resume_message_wait(process);

    if !block && *process.user_data() != Thread::ReadyToRun {
        debug_assert!(if let Thread::MessageWait(_) = process.user_data() { true } else { false });
        *process.user_data() = Thread::ReadyToRun;
        process.resume(Some(wasmi::RuntimeValue::I32(0)));
    }
}

/// If the given thread is waiting for a message to arrive, checks the queue and tries to resume
/// said thread.
fn try_resume_message_wait(thread: &mut processes::ProcessesCollectionThread<Process, Thread>) {
    if thread.process_user_data().messages_queue.is_empty() {
        return;
    }

    let msg_wait = match thread.user_data() {
        Thread::MessageWait(ref wait) => wait.clone(), // TODO: don't clone?
        _ => return,
    };

    // Try to find a message in the queue that matches something the user is waiting for.
    let mut index_in_queue = 0;
    let index_in_msg_ids = loop {
        if index_in_queue >= thread.process_user_data().messages_queue.len() {
            // No message found.
            return;
        }

        // For that message in queue, grab the value that must be in `msg_ids` in order to match.
        let msg_id = match &thread.process_user_data().messages_queue[index_in_queue] {
            syscalls::ffi::Message::Interface(_) => 1,
            syscalls::ffi::Message::Response(response) => {
                debug_assert!(response.message_id >= 2);
                response.message_id
            }
        };

        if let Some(p) = msg_wait.msg_ids.iter().position(|id| *id == msg_id) {
            break p as u32;
        }

        index_in_queue += 1;
    };

    // If we reach here, we have found a message that matches what the user wants.

    // Adjust the `index_in_list` field of the message to match what we have.
    match thread.process_user_data().messages_queue[index_in_queue] {
        syscalls::ffi::Message::Response(ref mut response) => {
            response.index_in_list = index_in_msg_ids;
        }
        syscalls::ffi::Message::Interface(ref mut interface) => {
            interface.index_in_list = index_in_msg_ids;
        }
    }

    // Turn said message into bytes.
    // TODO: would be great to not do that every single time
    let msg_bytes = thread.process_user_data().messages_queue[index_in_queue].encode();

    if msg_wait.out_size as usize >= msg_bytes.len() {
        // TODO: don't use as
        // Write the message in the process's memory.
        thread
            .write_memory(msg_wait.out_pointer, &msg_bytes)
            .unwrap();
        // Zero the corresponding entry in the messages to wait upon.
        thread
            .write_memory(msg_wait.msg_ids_ptr + index_in_msg_ids * 8, &[0; 8])
            .unwrap();
        // Pop the message from the queue, so that we don't deliver it twice.
        thread.process_user_data().messages_queue.remove(index_in_queue);
    }

    *thread.user_data() = Thread::ReadyToRun;
    thread.resume(Some(wasmi::RuntimeValue::I32(msg_bytes.len() as i32))); // TODO: don't use as
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
            (func $_start (result i32)
                i32.const 5)
            (export "_start" (func $_start)))
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
                assert_eq!(process, expected_pid);
                assert_eq!(return_value, Some(wasmi::RuntimeValue::I32(5)));
            }
            _ => panic!(),
        }
    }

    #[test]
    #[ignore] // TODO: test fails
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
            _ => panic!(),
        }
    }

    #[test]
    fn module_wait_extrinsic() {
        let module = Module::from_wat(
            r#"(module
            (import "foo" "test" (func $test (result i32)))
            (func $_start (result i32)
                call $test)
            (export "_start" (func $_start)))
        "#,
        )
        .unwrap();

        let mut core = Core::<u32>::new()
            .with_extrinsic("foo", "test", Signature::new(iter::empty(), None), 639u32)
            .build();

        let expected_pid = core.execute(&module).unwrap().pid();

        let thread_id = match core.run() {
            CoreRunOutcome::ThreadWaitExtrinsic {
                mut thread,
                extrinsic,
                params,
            } => {
                assert_eq!(thread.pid(), expected_pid);
                assert_eq!(*extrinsic, 639);
                assert!(params.is_empty());
                thread.id()
            }
            _ => panic!(),
        };

        core.thread_by_id(thread_id)
            .unwrap()
            .resolve_extrinsic_call(Some(wasmi::RuntimeValue::I32(713)));

        match core.run() {
            CoreRunOutcome::ProgramFinished {
                process,
                return_value,
            } => {
                assert_eq!(process, expected_pid);
                assert_eq!(return_value, Some(wasmi::RuntimeValue::I32(713)));
            }
            _ => panic!(),
        }
    }
}
