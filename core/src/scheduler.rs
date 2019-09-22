// Copyright(c) 2019 Pierre Krieger

use crate::interface::{Interface, InterfaceHash, InterfaceId};
use crate::module::Module;
use crate::signature::Signature;
use crate::sig;

use alloc::borrow::Cow;
use core::{convert::TryFrom, marker::PhantomData, ops::RangeBounds};
use hashbrown::{hash_map::Entry, HashMap, HashSet};

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
    interfaces: HashMap<InterfaceHash, Pid>,

    /// List of functions that processes can call.
    /// The key of this map is an arbitrary `usize` that we pass to the WASM interpreter.
    /// This field is never modified after the `Core` is created.
    extrinsics: HashMap<usize, Extrinsic<T>>,

    /// Map used to resolve imports when starting a process.
    /// For each module and function name, stores the signature and an arbitrary usize that
    /// corresponds to the entry in `extrinsics`.
    /// This field is never modified after the `Core` is created.
    extrinsics_id_assign: HashMap<(InterfaceId, Cow<'static, str>), (usize, Signature)>,
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
    Idle,
}

/// Additional information about a process.
struct Process {
    /// Data available for retrieval by the process.
    messages_queue: Vec<syscalls::ffi::Message>,

    /// List of messages that have been received and that need an answer. Contains the PID of the
    /// emitter of the message.
    // TODO: messages uniquely belong to a process
    messages_to_answer: Vec<(u64, Pid)>,
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
                            Entry::Vacant(e) => e.insert(process.pid()),
                        };
                        process.resume(Some(wasmi::RuntimeValue::I32(0)));
                    }
                    _ => unimplemented!()   // TODO:
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

    /// Start executing the module passed as parameter.
    ///
    /// Each import of the [`Module`](crate::module::Module) is resolved.
    pub fn execute(&mut self, module: &Module) -> Result<CoreProcess<T>, vm::NewErr> {
        let metadata = Process {
            messages_queue: Vec::new(),
            messages_to_answer: Vec::new(),
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

    /// Turns the builder into a [`Core`].
    pub fn build(mut self) -> Core<T> {
        // We're not going to modify these fields ever again, so let's free some memory.
        self.extrinsics.shrink_to_fit();
        self.extrinsics_id_assign.shrink_to_fit();
        debug_assert_eq!(self.extrinsics.len(), self.extrinsics_id_assign.len());

        Core {
            processes: processes::ProcessesCollection::new(),
            interfaces: HashMap::with_capacity(32),
            extrinsics: self.extrinsics,
            extrinsics_id_assign: self.extrinsics_id_assign,
        }
    }
}

// TODO:
fn poll_next_message(list: &mut [u64]) {
    unimplemented!()
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
