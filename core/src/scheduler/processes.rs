// Copyright(c) 2019 Pierre Krieger

use crate::interface::InterfaceId;
use crate::module::Module;
use crate::scheduler::{
    pid::{Pid, PidPool},
    process,
};
use core::ops::RangeBounds;
use hashbrown::{HashMap, hash_map::{DefaultHashBuilder, Entry, OccupiedEntry}};

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

/// Single running process in the list.
struct Process<T> {
    /// State of a single process.
    state_machine: process::ProcessStateMachine,
    /// User-chosen data (opaque to us) that describes the process.
    user_data: T,
    /// Value to use when resuming. If `Some`, the process is ready for a round of running. If
    /// `None`, then we're waiting for the user to call `resume`.
    value_back: Option<Option<wasmi::RuntimeValue>>,
}

/// Access to a process within the collection.
pub struct ProcessesCollectionProc<'a, T> {
    /// Pointer within the hashmap.
    process: OccupiedEntry<'a, Pid, Process<T>, DefaultHashBuilder>,
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
        Ok(self.process_by_id(new_pid).unwrap())
    }

    pub fn run(&mut self) -> RunOneOutcome<T> {
        // We start by finding an element in `self.processes`.
        let mut process: OccupiedEntry<_, _, _> = {
            let entry = self.processes.iter_mut().find(|(_, p)| p.is_ready_to_run()).map(|(k, _)| k.clone());
            match entry {
                Some(pid) => match self.processes.entry(pid) {
                    Entry::Occupied(p) => p,
                    Entry::Vacant(_) => unreachable!()
                },
                None => return RunOneOutcome::Idle,
            }
        };

        let value_back = process.get_mut().value_back.take().unwrap();
        match process.get_mut().state_machine.resume(value_back) {
            Err(process::ResumeErr::BadValueTy { .. }) => panic!(), // TODO:
            Ok(process::ExecOutcome::Finished(value)) => RunOneOutcome::Finished {
                process: ProcessesCollectionProc { process },
                value,
            },
            Ok(process::ExecOutcome::Interrupted { id, params }) => RunOneOutcome::Interrupted {
                process: ProcessesCollectionProc { process },
                id,
                params,
            },
            Ok(process::ExecOutcome::Errored(error)) => {
                let (pid, Process { user_data, .. }) = process.remove_entry();
                RunOneOutcome::Errored {
                    pid,
                    user_data,
                    error,
                }
            }
        }
    }

    /// Returns a process by its [`Pid`], if it exists.
    pub fn process_by_id(&mut self, pid: Pid) -> Option<ProcessesCollectionProc<T>> {
        match self.processes.entry(pid) {
            Entry::Occupied(e) => Some(ProcessesCollectionProc {
                process: e,
            }),
            Entry::Vacant(_) => None,
        }
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
        self.process.key()
    }

    pub fn into_user_data(self) -> &'a mut T {
        &mut self.process.into_mut().user_data
    }

    pub fn user_data(&mut self) -> &mut T {
        &mut self.process.get_mut().user_data
    }

    pub fn resume(&mut self, value: Option<wasmi::RuntimeValue>) {
        // TODO: check type of the value?
        if self.process.get_mut().value_back.is_some() {
            panic!()
        }

        self.process.get_mut().value_back = Some(value);
    }

    // TODO: adjust to final API
    pub fn read_memory(&mut self, range: impl RangeBounds<usize>) -> Result<Vec<u8>, ()> {
        self.process.get_mut().state_machine.read_memory(range)
    }
}
