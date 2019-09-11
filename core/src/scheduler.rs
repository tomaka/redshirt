// Copyright(c) 2019 Pierre Krieger

use crate::interface::{Interface, InterfaceId, InterfaceHash};
use crate::module::Module;
use crate::signature::Signature;

use alloc::{borrow::Cow, collections::VecDeque};
use bimap::BiHashMap;
use hashbrown::{HashMap, HashSet, hash_map::Entry};
use self::pid::PidPool;

mod pid;
mod process;

// TODO: move definition?
pub use self::pid::Pid;

pub struct Core<T> {
    pid_pool: PidPool,
    loaded: HashMap<Pid, Program>,

    extrinsics: HashMap<((InterfaceId, Cow<'static, str>)), (T, Signature)>,

    /// For each interface, its definition and which program is fulfilling it.
    interfaces: HashMap<InterfaceId, (Pid, Interface)>,

    /// Holds a bijection between arbitrary values (the `usize` on the left side) that we pass
    /// to the WASM interpreter, and the function that corresponds to it.
    /// Whenever the interpreter wants to link to a function, we look for the `usize` corresponding
    /// to the requested function. When the interpreter wants to execute that function, it passes
    /// back that `usize` to us, and we can look which function it is.
    externals_indices: BiHashMap<usize, (InterfaceId, Cow<'static, str>)>,

    /// Queue of tasks to execute.
    scheduled: VecDeque<Scheduled>,
}

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
        return_value: Option<wasmi::RuntimeValue>,      // TODO: force to i32?
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
    // TODO: temporary; remove
    Nothing,
}

struct Program {
    state_machine: process::ProcessStateMachine,
    depends_on: Vec<Pid>,
    depended_on: HashSet<Pid>,

    /// If `Some`, then after this execution finishes we must schedule `Pid` and feed the value
    /// back to it. The `Program` corresponding to `Pid` **must** have `execution` set to `Some`.
    feed_value_to: Option<Pid>,
}

/// Task scheduled for execution.
struct Scheduled {
    /// Program scheduled for execution. It **must** have `execution` set to `Some`.
    pid: Pid,

    /// Value to pass back when resuming execution.
    resume_value: Option<wasmi::RuntimeValue>,
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

    /// Returns a `Future` that runs the core.
    ///
    /// This returns a `Future` so that it is possible to interrupt the process.
    // TODO: make multithreaded
    #[allow(clippy::needless_lifetimes)]        // TODO: lifetime necessary because of async/await
    pub async fn run<'a>(&'a mut self) -> CoreRunOutcome<'a, T> {
        // TODO: wasi doesn't allow interrupting executions
        while let Some(scheduled) = self.scheduled.pop_front() {
            let program = self.loaded.get_mut(&scheduled.pid).unwrap();
            match program.state_machine.resume(scheduled.resume_value) {
                process::ExecOutcome::Finished(val) => {
                    if let Some(feed_value_to) = program.feed_value_to.take() {
                        self.scheduled.push_back(Scheduled {
                            pid: feed_value_to,
                            resume_value: val,
                        });
                    } else {
                        return CoreRunOutcome::ProgramFinished {
                            pid: scheduled.pid,
                            return_value: val,
                        };
                    }
                }
                process::ExecOutcome::Interrupted { id, params } => {
                    let (interface, function) = self.externals_indices.get_by_left(&id).unwrap();
                    // TODO: check params against signature? is that necessary? maybe a debug_assert!
                    if let Some((extrinsic, _)) = self.extrinsics.get(&(interface.clone(), function.clone())) {
                        return CoreRunOutcome::ProgramWaitExtrinsic {
                            pid: scheduled.pid,
                            extrinsic,
                            params,
                        };
                    }
                }
                process::ExecOutcome::Errored(trap) => {
                    println!("oops, actual error! {:?}", trap);
                    // TODO: remove program from list and return `ProgramCrashed` event
                }
            }
        }

        // TODO: sleep or something instead of terminating the future
        CoreRunOutcome::Nothing
    }

    /// After `ProgramWaitExtrinsic` has been returned, you have to call this method in order to
    /// inject back the result of the extrinsic call.
    // TODO: don't expose wasmi::RuntimeValue
    pub fn resolve_extrinsic_call(&mut self, pid: Pid, return_value: Option<wasmi::RuntimeValue>) {
        // TODO: check if that's correct
        self.scheduled.push_back(Scheduled {
            pid,
            resume_value: return_value,
        });
    }

    /// Sets the process that fulfills the given interface.
    ///
    /// Returns an error if there is already a process fulfilling the given interface.
    pub fn set_interface_provider(&mut self, interface: Interface, pid: Pid) -> Result<(), ()> {
        if self.extrinsics.keys().any(|(i, _)| i == &InterfaceId::Hash(interface.hash().clone())) {       // TODO: more efficient way?
            return Err(())
        }

        match self.interfaces.entry(InterfaceId::Hash(interface.hash().clone())) {
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
        let state_machine = process::ProcessStateMachine::new(module, |interface, function, signature| {
            if let Some(index) = self.externals_indices.get_by_right(&(interface.clone(), function.into())) {
                // TODO: check signature validity
                return Ok(*index);
            }
            
            // TODO: also check interfaces dependencies
            if let Some((_, expected_sig)) = self.extrinsics.get(&(interface.clone(), function.into())) {
                if !expected_sig.matches_wasmi(signature) {
                    return Err(());
                }

                let index = self.externals_indices.len();
                self.externals_indices.insert(index, (interface.clone(), function.to_owned().into()));
                return Ok(index);
            }

            if let Some((provider_pid, interface_def)) = self.interfaces.get(&interface) {
                // TODO: check function existance and signature validity against interface_def
                let index = self.externals_indices.len();
                self.externals_indices.insert(index, (interface.clone(), function.to_owned().into()));
                return Ok(index);
            }

            Err(())
        })?;

        // We don't modify `self` until after we started the state machine.
        let pid = self.pid_pool.assign();
        let schedule_me = state_machine.is_executing();
        self.loaded.insert(pid, Program {
            state_machine,
            depends_on: Vec::new(),
            depended_on: HashSet::new(),
            feed_value_to: None,
        });
        if schedule_me {
            self.scheduled.push_back(Scheduled {
                pid,
                resume_value: None,
            });
        }
        Ok(pid)
    }
}

impl<T> CoreBuilder<T> {
    pub fn with_extrinsic(mut self, interface: impl Into<InterfaceId>, f_name: impl Into<Cow<'static, str>>, signature: &Signature, token: impl Into<T>) -> Self {
        // TODO: panic if we already have it
        let interface = interface.into();
        let f_name = f_name.into();

        self.extrinsics.insert((interface.clone(), f_name.clone()), (token.into(), signature.clone()));
        let index = self.externals_indices.len();
        debug_assert!(!self.externals_indices.contains_left(&index));
        self.externals_indices.insert(index, (interface, f_name));
        self
    }

    pub fn build(mut self) -> Core<T> {
        // We're not going to modify `extrinsics` ever again, so let's free some memory.
        self.extrinsics.shrink_to_fit();

        Core {
            pid_pool: PidPool::new(),
            loaded: HashMap::with_capacity(128),
            extrinsics: self.extrinsics,
            interfaces: HashMap::with_capacity(32),
            externals_indices: self.externals_indices,
            scheduled: VecDeque::with_capacity(32),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{module::Module, signature::Signature};
    use super::{Core, CoreRunOutcome};
    use core::iter;

    #[test]
    fn basic_module() {
        let module = Module::from_wat(r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                i32.const 5)
            (export "main" (func $main)))
        "#).unwrap();

        let mut core = Core::<!>::new().build();
        let expected_pid = core.execute(&module).unwrap();

        let outcome = futures::executor::block_on(core.run());
        match outcome {
            CoreRunOutcome::ProgramFinished { pid, return_value } => {
                assert_eq!(pid, expected_pid);
                assert_eq!(return_value, Some(wasmi::RuntimeValue::I32(5)));
            }
            _ => panic!()
        }
    }

    #[test]
    #[ignore]       // TODO:
    fn trapping_module() {
        let module = Module::from_wat(r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                unreachable)
            (export "main" (func $main)))
        "#).unwrap();

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
        let module = Module::from_wat(r#"(module
            (import "" "test" (func $test (result i32)))
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                call $test)
            (export "main" (func $main)))
        "#).unwrap();

        let mut core = Core::<u32>::new()
            .with_extrinsic([0; 32], "test", &Signature::new(iter::empty(), None), 639u32)
            .build();

        let expected_pid = core.execute(&module).unwrap();

        let outcome = futures::executor::block_on(core.run());
        match outcome {
            CoreRunOutcome::ProgramWaitExtrinsic { pid, extrinsic, params } => {
                assert_eq!(pid, expected_pid);
                assert_eq!(*extrinsic, 639);
                assert!(params.is_empty());
            }
            _ => panic!()
        }

        core.resolve_extrinsic_call(expected_pid, Some(wasmi::RuntimeValue::I32(713)));

        let outcome = futures::executor::block_on(core.run());
        match outcome {
            CoreRunOutcome::ProgramFinished { pid, return_value } => {
                assert_eq!(pid, expected_pid);
                assert_eq!(return_value, Some(wasmi::RuntimeValue::I32(713)));
            }
            _ => panic!()
        }
    }
}
