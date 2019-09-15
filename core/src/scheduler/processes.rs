// Copyright(c) 2019 Pierre Krieger

use crate::interface::InterfaceId;
use crate::module::Module;
use crate::scheduler::{
    pid::{Pid, PidPool},
    process,
};
use core::ops::RangeBounds;
use hashbrown::HashMap;

/// Collection of multiple [`ProcessStateMachine`]s grouped together in a smart way.
///
/// This struct handles interleaving processes execution.
///
/// The generic parameter is a "user data" that is stored per process and allows the user to put
/// extra information associated to a process.
pub struct ProcessesCollection<T> {
    /// Allocations of process IDs.
    pid_pool: PidPool,

    /// List of running processes.
    processes: HashMap<Pid, Process<T>>,
}

struct Process<T> {
    state_machine: process::ProcessStateMachine,
    user_data: T,
    value_back: Option<Option<wasmi::RuntimeValue>>,
}

pub struct ProcessesCollectionProc<'a, T> {
    pid: Pid,
    process: &'a mut Process<T>,
}

/// Outcome of the [`run`](ProcessesCollection::run) function.
// TODO: Debug
pub enum RunOneOutcome<'a, T> {
    /// The currently-executed function in a process has finished.
    ///
    /// The process is now inactive.
    Finished {
        /// Process that has finished.
        process: ProcessesCollectionProc<'a, T>,
        /// Value returned by the function that was executed.
        value: Option<wasmi::RuntimeValue>,
    },

    /// The currently-executed function has been paused due to a call to an external function.
    ///
    /// This variant contains the identifier of the external function that is expected to be
    /// called, and its parameters. When you call [`resume`](ProcessesCollectionProc::resume) again,
    /// you must pass back the outcome of calling that function.
    // TODO: more docs
    Interrupted {
        /// Process that has been interrupted.
        process: ProcessesCollectionProc<'a, T>,

        /// Identifier of the function to call. Corresponds to the value provided at
        /// initialization when resolving imports.
        id: usize,

        /// Parameters of the function call.
        params: Vec<wasmi::RuntimeValue>,
    },

    /// The currently-executed function has finished with an error. The process has been destroyed.
    Errored {
        /// Pid of the process that has been destroyed.
        pid: Pid,
        /// User data that belonged to the process.
        user_data: T,
        /// Error that happened.
        // TODO: error type should change here
        error: wasmi::Trap,
    },

    /// No process is ready to run. Nothing was done.
    Idle,
}

/// Minimum capacity of the container of the list of processes.
///
/// If we shrink the container too much, then it will have to perform lots of allocations in order
/// to grow again in the future. We therefore avoid that situation.
const PROCESSES_MIN_CAPACITY: usize = 128;

impl<T> ProcessesCollection<T> {
    pub fn new() -> Self {
        ProcessesCollection {
            pid_pool: PidPool::new(),
            processes: HashMap::with_capacity(PROCESSES_MIN_CAPACITY),
        }
    }

    pub fn execute(
        &mut self,
        module: &Module,
        user_data: T,
        mut symbols: impl FnMut(&InterfaceId, &str, &wasmi::Signature) -> Result<usize, ()>,
    ) -> Result<ProcessesCollectionProc<T>, ()> {
        let state_machine = process::ProcessStateMachine::new(module, symbols)?;
        let has_main = state_machine.is_executing();

        // We only modify `self` at the very end.
        let new_pid = self.pid_pool.assign();
        self.processes.insert(
            new_pid,
            Process {
                state_machine,
                user_data,
                value_back: if has_main { Some(None) } else { None },
            },
        );
        // Shrink the list from time to time so that it doesn't grow too much.
        if u64::from(new_pid) % 256 == 0 {
            self.processes.shrink_to(PROCESSES_MIN_CAPACITY);
        }
        Ok(self.process_by_id(&new_pid).unwrap())
    }

    pub fn run(&mut self) -> RunOneOutcome<T> {
        // We start by finding an element in `self.processes`.
        let (pid, process) = {
            let entry = self.processes.iter_mut().find(|(_, p)| p.is_ready_to_run());
            match entry {
                Some(e) => e,
                None => return RunOneOutcome::Idle,
            }
        };

        let value_back = process.value_back.take().unwrap();
        match process.state_machine.resume(value_back) {
            Err(process::ResumeErr::BadValueTy { .. }) => panic!(), // TODO:
            Ok(process::ExecOutcome::Finished(value)) => RunOneOutcome::Finished {
                process: ProcessesCollectionProc { pid: *pid, process },
                value,
            },
            Ok(process::ExecOutcome::Interrupted { id, params }) => RunOneOutcome::Interrupted {
                process: ProcessesCollectionProc { pid: *pid, process },
                id,
                params,
            },
            Ok(process::ExecOutcome::Errored(error)) => {
                let pid_val = *pid;
                drop((pid, process));
                // FIXME: remove process from list
                unimplemented!()
            }
        }
    }

    pub fn process_by_id(&mut self, pid: &Pid) -> Option<ProcessesCollectionProc<T>> {
        self.processes
            .get_mut(pid)
            .map(|p| ProcessesCollectionProc {
                pid: *pid,
                process: p,
            })
    }
}

impl<T> Default for ProcessesCollection<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Process<T> {
    fn is_ready_to_run(&self) -> bool {
        match self {
            Process {
                value_back: Some(_),
                ..
            } => true,
            _ => false,
        }
    }
}

impl<'a, T> ProcessesCollectionProc<'a, T> {
    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollection::process_by_id).
    pub fn pid(&self) -> &Pid {
        &self.pid
    }

    pub fn into_user_data(self) -> &'a mut T {
        &mut self.process.user_data
    }

    pub fn user_data(&mut self) -> &mut T {
        &mut self.process.user_data
    }

    pub fn resume(&mut self, value: Option<wasmi::RuntimeValue>) {
        // TODO: check type of the value?
        if self.process.value_back.is_some() {
            panic!()
        }

        self.process.value_back = Some(value);
    }

    // TODO: adjust to final API
    pub fn read_memory(&self, range: impl RangeBounds<usize>) -> Result<Vec<u8>, ()> {
        self.process.state_machine.read_memory(range)
    }
}
