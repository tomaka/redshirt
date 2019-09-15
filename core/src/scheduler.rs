// Copyright(c) 2019 Pierre Krieger

use crate::interface::{Interface, InterfaceHash, InterfaceId};
use crate::module::Module;
use crate::signature::Signature;

use self::pid::PidPool;
use alloc::{borrow::Cow, collections::VecDeque};
use bimap::BiHashMap;
use core::ops::RangeBounds;
use hashbrown::{hash_map::Entry, HashMap, HashSet};

mod pid;
mod processes;
mod vm;

// TODO: move definition?
pub use self::pid::Pid;

pub struct Core<T> {
    /// List of running processes.
    processes: processes::ProcessesCollection<Process>,

    /// List of functions available to processes that are handled by the user of this struct.
    extrinsics: HashMap<((InterfaceId, Cow<'static, str>)), (T, Signature)>,

    /// For each interface, its definition and which program is fulfilling it.
    interfaces: HashMap<InterfaceId, (Pid, Interface)>,

    /// Holds a bijection between arbitrary values (the `usize` on the left side) that we pass
    /// to the WASM interpreter, and the function that corresponds to it.
    /// Whenever the interpreter wants to link to a function, we look for the `usize` corresponding
    /// to the requested function. When the interpreter wants to execute that function, it passes
    /// back that `usize` to us, and we can look which function it is.
    externals_indices: BiHashMap<usize, (InterfaceId, Cow<'static, str>)>,
}

/// Prototype for a `Core` under construction.
pub struct CoreBuilder<T> {
    /// See the corresponding field in `Core`.
    extrinsics: HashMap<((InterfaceId, Cow<'static, str>)), (T, Signature)>,
    /// See the corresponding field in `Core`.
    externals_indices: BiHashMap<usize, (InterfaceId, Cow<'static, str>)>,
}

#[derive(Debug)]
pub enum CoreRunOutcome<'a, T> {
    ProgramFinished {
        pid: Pid,
        return_value: Option<wasmi::RuntimeValue>, // TODO: force to i32?
    },
    ProgramCrashed {
        pid: Pid,
        error: wasmi::Error,
    },
    ProgramWaitExtrinsic {
        pid: Pid,
        extrinsic: &'a T,
        params: Vec<wasmi::RuntimeValue>,
    },
    Idle,
}

struct Process {
    depends_on: Vec<Pid>,
    depended_on: HashSet<Pid>,

    /// If `Some`, then after this execution finishes we must schedule `Pid` and feed the value
    /// back to it.
    feed_value_to: Option<Pid>,
}

impl<T> Core<T> {
    /// Initialies a new `Core`.
    pub fn new() -> CoreBuilder<T> {
        CoreBuilder {
            extrinsics: HashMap::new(),
            externals_indices: BiHashMap::new(),
        }
    }

    pub fn has_interface(&self, interface: InterfaceId) -> bool {
        self.interfaces.contains_key(&interface)
    }

    /// Kills the given process immediately.
    ///
    /// Returns an error if the given `pid` isn't valid or isn't valid anymore.
    pub fn abort_process(&mut self, pid: Pid) -> Result<(), ()> {
        // TODO: implement
        panic!("aborting {:?}", pid);
        Ok(())
    }

    /// Run the core once.
    // TODO: make multithreaded
    pub fn run(&mut self) -> CoreRunOutcome<T> {
        match self.processes.run() {
            processes::RunOneOutcome::Finished { mut process, value } => {
                if let Some(feed_value_to) = process.user_data().feed_value_to.take() {
                    self.processes
                        .process_by_id(feed_value_to)
                        .unwrap()
                        .resume(value);
                } else {
                    return CoreRunOutcome::ProgramFinished {
                        pid: *process.pid(),
                        return_value: value,
                    };
                }
            }
            processes::RunOneOutcome::Interrupted {
                process,
                id,
                params,
            } => {
                let (interface, function) = self.externals_indices.get_by_left(&id).unwrap();
                // TODO: check params against signature? is that necessary? maybe a debug_assert!
                if let Some((extrinsic, _)) =
                    self.extrinsics.get(&(interface.clone(), function.clone()))
                {
                    return CoreRunOutcome::ProgramWaitExtrinsic {
                        pid: *process.pid(),
                        extrinsic,
                        params,
                    };
                }
            }
            processes::RunOneOutcome::Errored {
                pid,
                user_data,
                error,
            } => {
                println!("oops, actual error! {:?}", error);
                // TODO: remove program from list and return `ProgramCrashed` event
            }
            processes::RunOneOutcome::Idle => {}
        }

        CoreRunOutcome::Idle
    }

    /// After `ProgramWaitExtrinsic` has been returned, you have to call this method in order to
    /// inject back the result of the extrinsic call.
    // TODO: don't expose wasmi::RuntimeValue
    pub fn resolve_extrinsic_call(&mut self, pid: Pid, return_value: Option<wasmi::RuntimeValue>) {
        // TODO: check if the value type is correct
        self.processes
            .process_by_id(pid)
            .unwrap()
            .resume(return_value);
    }

    /// Sets the process that fulfills the given interface.
    ///
    /// Returns an error if there is already a process fulfilling the given interface.
    pub fn set_interface_provider(&mut self, interface: Interface, pid: Pid) -> Result<(), ()> {
        if self
            .extrinsics
            .keys()
            .any(|(i, _)| i == &InterfaceId::Hash(interface.hash().clone()))
        {
            // TODO: more efficient way?
            return Err(());
        }

        match self
            .interfaces
            .entry(InterfaceId::Hash(interface.hash().clone()))
        {
            Entry::Occupied(_) => Err(()),
            Entry::Vacant(e) => {
                e.insert((pid, interface));
                Ok(())
            }
        }
    }

    /// Start executing the module passed as parameter.
    ///
    /// Each import of the [`Module`](crate::module::Module) is resolved.
    pub fn execute(&mut self, module: &Module) -> Result<Pid, ()> {
        let metadata = Process {
            depends_on: Vec::new(),
            depended_on: HashSet::default(),
            feed_value_to: None,
        };

        let mut externals_indices = &mut self.externals_indices;
        let interfaces = &mut self.interfaces;
        let extrinsics = &mut self.extrinsics;

        let process =
            self.processes
                .execute(module, metadata, move |interface, function, signature| {
                    if let Some(index) =
                        externals_indices.get_by_right(&(interface.clone(), function.into()))
                    {
                        // TODO: check signature validity
                        return Ok(*index);
                    }

                    // TODO: also check interfaces dependencies
                    if let Some((_, expected_sig)) =
                        extrinsics.get(&(interface.clone(), function.into()))
                    {
                        if !expected_sig.matches_wasmi(signature) {
                            return Err(());
                        }

                        let index = externals_indices.len();
                        externals_indices
                            .insert(index, (interface.clone(), function.to_owned().into()));
                        return Ok(index);
                    }

                    if let Some((provider_pid, interface_def)) = interfaces.get(&interface) {
                        // TODO: check function existance and signature validity against interface_def
                        let index = externals_indices.len();
                        externals_indices
                            .insert(index, (interface.clone(), function.to_owned().into()));
                        return Ok(index);
                    }

                    Err(())
                })?;

        Ok(*process.pid())
    }

    /// Copies the given memory range of the given process into a `Vec<u8>`.
    ///
    /// Returns an error if the range is invalid.
    // TODO: should really return &mut [u8] I think
    pub fn read_memory(&mut self, pid: Pid, range: impl RangeBounds<usize>) -> Result<Vec<u8>, ()> {
        self.processes
            .process_by_id(pid)
            .unwrap()
            .read_memory(range)
    }
}

impl<T> CoreBuilder<T> {
    pub fn with_extrinsic(
        mut self,
        interface: impl Into<InterfaceId>,
        f_name: impl Into<Cow<'static, str>>,
        signature: &Signature,
        token: impl Into<T>,
    ) -> Self {
        // TODO: panic if we already have it
        let interface = interface.into();
        let f_name = f_name.into();

        self.extrinsics.insert(
            (interface.clone(), f_name.clone()),
            (token.into(), signature.clone()),
        );
        let index = self.externals_indices.len();
        debug_assert!(!self.externals_indices.contains_left(&index));
        self.externals_indices.insert(index, (interface, f_name));
        self
    }

    pub fn build(mut self) -> Core<T> {
        // We're not going to modify `extrinsics` ever again, so let's free some memory.
        self.extrinsics.shrink_to_fit();

        Core {
            processes: processes::ProcessesCollection::new(),
            extrinsics: self.extrinsics,
            interfaces: HashMap::with_capacity(32),
            externals_indices: self.externals_indices,
        }
    }
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
        let expected_pid = core.execute(&module).unwrap();

        match core.run() {
            CoreRunOutcome::ProgramFinished { pid, return_value } => {
                assert_eq!(pid, expected_pid);
                assert_eq!(return_value, Some(wasmi::RuntimeValue::I32(5)));
            }
            _ => panic!(),
        }
    }

    #[test]
    #[ignore] // TODO:
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
        let expected_pid = core.execute(&module).unwrap();

        /*let outcome = futures::executor::block_on(core.run());
        match outcome {
            CoreRunOutcome::ProgramCrashed { pid, .. } => {
                assert_eq!(pid, expected_pid);
            }
            _ => panic!()
        }*/
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

        let expected_pid = core.execute(&module).unwrap();

        match core.run() {
            CoreRunOutcome::ProgramWaitExtrinsic {
                pid,
                extrinsic,
                params,
            } => {
                assert_eq!(pid, expected_pid);
                assert_eq!(*extrinsic, 639);
                assert!(params.is_empty());
            }
            _ => panic!(),
        }

        core.resolve_extrinsic_call(expected_pid, Some(wasmi::RuntimeValue::I32(713)));

        match core.run() {
            CoreRunOutcome::ProgramFinished { pid, return_value } => {
                assert_eq!(pid, expected_pid);
                assert_eq!(return_value, Some(wasmi::RuntimeValue::I32(713)));
            }
            _ => panic!(),
        }
    }
}
