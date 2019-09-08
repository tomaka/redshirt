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

pub struct Core {
    pid_pool: PidPool,
    loaded: HashMap<Pid, Program>,

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

impl Core {
    /// Initialies a new `Core`.
    pub fn new() -> Core {
        Core {
            pid_pool: PidPool::new(),
            loaded: HashMap::with_capacity(128),
            interfaces: HashMap::with_capacity(32),
            externals_indices: BiHashMap::with_capacity(128),
            scheduled: VecDeque::with_capacity(32),
        }
    }

    pub fn has_interface(&self, interface: InterfaceHash) -> bool {
        self.interfaces.contains_key(&interface)
    }

    /// Returns a `Future` that runs the core.
    ///
    /// This returns a `Future` so that it is possible to interrupt the process.
    // TODO: make multithreaded
    pub async fn run(&mut self) {
        // TODO: wasi doesn't allow interrupting executions
        while let Some(mut scheduled) = self.scheduled.pop_front() {
            let program = self.loaded.get_mut(&scheduled.pid).unwrap();
            match program.state_machine.resume(scheduled.resume_value) {
                process::ExecOutcome::Finished(val) => {
                    if let Some(feed_value_to) = program.feed_value_to.take() {
                        self.scheduled.push_back(Scheduled {
                            pid: feed_value_to,
                            resume_value: val,
                        });
                    } else {
                        println!("finished exec => {:?}", val);
                    }
                }
                process::ExecOutcome::Interrupted(index, arguments) => {
                    let (interface, function) = self.externals_indices.get_by_left(&index).unwrap();
                    // TODO: prototype hack
                    println!("{:?} {:?} {:?}", interface, function, arguments);
                    scheduled.resume_value = Some(wasmi::RuntimeValue::I32(7));
                    self.scheduled.push_back(scheduled);
                }
                process::ExecOutcome::Errored(trap) => {
                    println!("oops, actual error!");
                }
            }
        }

        // TODO: sleep or something instead of terminating the future
    }

    /// Start executing the module passed as parameter.
    pub fn execute(&mut self, module: &Module) -> Result<Pid, ()> {
        let state_machine = process::ProcessStateMachine::new(module, |interface, function, signature| {
            // TODO: check signature validity
            if let Some(index) = self.externals_indices.get_by_right(&(interface.clone(), function.into())) {
                Ok(*index)
            } else {
                // TODO: first check whether the interface is fufilled by a module
                let index = self.externals_indices.len();
                self.externals_indices.insert(index, (interface.clone(), function.to_owned().into()));
                Ok(index)
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

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}
