// Copyright(c) 2019 Pierre Krieger

use crate::interface::InterfaceHash;
use crate::module::Module;

use alloc::{borrow::Cow, collections::VecDeque};
use bimap::BiHashMap;
use hashbrown::{HashMap, HashSet};
use self::pid::{Pid, PidPool};
use std::fmt;

mod builder;
mod pid;
mod process;

pub struct Core<T> {
    pid_pool: PidPool,
    loaded: HashMap<Pid, Program>,

    extrinsics: HashMap<((InterfaceHash, Cow<'static, str>)), T>,

    /// For each interface, which program is fulfilling it.
    interfaces: HashMap<InterfaceHash, Pid>,

    /// Holds a bijection between arbitrary values (the `usize` on the left side) that we pass
    /// to the WASM interpreter, and the function that corresponds to it.
    /// Whenever the interpreter wants to link to a function, we look for the `usize` corresponding
    /// to the requested function. When the interpreter wants to execute that function, it passes
    /// back that `usize` to us, and we can look which function it is.
    externals_indices: BiHashMap<usize, (InterfaceHash, Cow<'static, str>)>,

    /// Queue of tasks to execute.
    scheduled: VecDeque<Scheduled>,
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
    pub fn new() -> Core<T> {
        Core {
            pid_pool: PidPool::new(),
            loaded: HashMap::with_capacity(128),
            extrinsics: HashMap::with_capacity(16),
            interfaces: HashMap::with_capacity(32),
            externals_indices: BiHashMap::with_capacity(128),
            scheduled: VecDeque::with_capacity(32),
        }
    }

    // TODO: consider passing this in a builder instead of adding them afterwards
    pub fn register_extrinsic(&mut self, interface: impl Into<InterfaceHash>, f_name: impl Into<Cow<'static, str>>, token: impl Into<T>) {
        self.extrinsics.insert((interface.into(), f_name.into()), token.into());
    }

    pub fn has_interface(&self, interface: InterfaceHash) -> bool {
        self.interfaces.contains_key(&interface)
    }

    /// Returns a `Future` that runs the core.
    ///
    /// This returns a `Future` so that it is possible to interrupt the process.
    // TODO: make multithreaded
    // TODO: shouldn't return an Option but a plain value
    #[allow(clippy::needless_lifetimes)]        // TODO: necessary because of async/await
    pub async fn run<'a>(&'a mut self) -> RunOutcome<'a, T> {
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
                        return RunOutcome::ProgramFinished {
                            pid: scheduled.pid,
                            return_value: val,
                        };
                    }
                }
                process::ExecOutcome::Interrupted(index, arguments) => {
                    let (interface, function) = self.externals_indices.get_by_left(&index).unwrap();
                    if let Some(extrinsic) = self.extrinsics.get(&(interface.clone(), function.clone())) {
                        return RunOutcome::ProgramWaitExtrinsic {
                            pid: scheduled.pid,
                            extrinsic,
                        };
                    }
                }
                process::ExecOutcome::Errored(trap) => {
                    println!("oops, actual error!");
                    // TODO: remove program from list and return `ProgramCrashed` event
                }
            }
        }

        // TODO: sleep or something instead of terminating the future
        RunOutcome::Nothing
    }

    /// After `ProgramWaitExtrinsic` has been called, you have to call this method in order to
    /// inject back the result of the extrinsic call.
    pub fn resolve_extrinsic_call(&mut self, pid: Pid, return_value: Option<wasmi::RuntimeValue>) {
        // TODO: check if that's correct
        self.scheduled.push_back(Scheduled {
            pid,
            resume_value: return_value,
        });
    }

    /// Start executing the module passed as parameter.
    pub fn execute(&mut self, module: &Module) -> Result<Pid, ()> {
        let state_machine = process::ProcessStateMachine::new(module, |interface, function, signature| {
            // TODO: check signature validity
            if let Some(index) = self.externals_indices.get_by_right(&(interface.clone(), function.into())) {
                Ok(*index)
            } else {
                // TODO: also check interfaces dependencies
                if self.extrinsics.contains_key(&(interface.clone(), function.into())) {
                    let index = self.externals_indices.len();
                    self.externals_indices.insert(index, (interface.clone(), function.to_owned().into()));
                    Ok(index)
                } else {
                    Err(())
                }
            }
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

impl<T> Default for Core<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum RunOutcome<'a, T> {
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
    },
    // TODO: temporary; remove
    Nothing,
}

#[cfg(test)]
mod tests {
    use crate::module::Module;
    use super::{Core, RunOutcome};

    #[test]
    fn basic_module() {
        let module = Module::from_wat(r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                i32.const 5)
            (export "main" (func $main)))
        "#).unwrap();

        let mut core = Core::<!>::new();
        let expected_pid = core.execute(&module).unwrap();

        let outcome = futures::executor::block_on(core.run());
        match outcome {
            RunOutcome::ProgramFinished { pid, return_value } => {
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

        let mut core = Core::<!>::new();
        let expected_pid = core.execute(&module).unwrap();

        /*let outcome = futures::executor::block_on(core.run());
        match outcome {
            RunOutcome::ProgramCrashed { pid, .. } => {
                assert_eq!(pid, expected_pid);
            }
            _ => panic!()
        }*/
    }
}
