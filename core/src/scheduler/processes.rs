// Copyright(c) 2019 Pierre Krieger

use crate::interface::InterfaceId;
use crate::module::Module;
use crate::scheduler::{
    pid::{Pid, PidPool},
    vm,
};
use core::ops::RangeBounds;
use hashbrown::{
    hash_map::{DefaultHashBuilder, Entry, OccupiedEntry},
    HashMap,
};

/// Collection of multiple [`ProcessStateMachine`]s grouped together in a smart way.
///
/// This struct handles interleaving processes execution.
///
/// The generic parameters are "user data"s that are stored per process and per thread, and allows
/// the user to put extra information associated to a process or a thread.
pub struct ProcessesCollection<TPud, TTud> {
    /// Allocations of process IDs.
    pid_pool: PidPool,

    /// Identifier to assign to the next thread we create.
    next_thread_id: ThreadId,

    /// List of running processes.
    processes: HashMap<Pid, Process<TPud, TTud>>,
}

/// Identifier of a thread within the [`ProcessesCollection`].
///
/// No two threads share the same ID, even between multiple processes.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ThreadId(u64);

/// Single running process in the list.
struct Process<TPud, TTud> {
    /// State of a single process.
    state_machine: vm::ProcessStateMachine<Thread<TTud>>,

    /// User-chosen data (opaque to us) that describes the process.
    user_data: TPud,
}

/// Additional data associated to a thread.
struct Thread<TTud> {
    /// User-chosen data (opaque to us) that describes the thread.
    user_data: TTud,

    /// Identifier of the thread.
    thread_id: ThreadId,

    /// Value to use when resuming. If `Some`, the process is ready for a round of running. If
    /// `None`, then we're waiting for the user to call `resume`.
    value_back: Option<Option<wasmi::RuntimeValue>>,
}

/// Access to a process within the collection.
pub struct ProcessesCollectionProc<'a, TPud, TTud> {
    /// Pointer within the hashmap.
    process: OccupiedEntry<'a, Pid, Process<TPud, TTud>, DefaultHashBuilder>,
}

/// Access to a thread within the collection.
pub struct ProcessesCollectionThread<'a, TPud, TTud> {
    /// Pointer within the hashmap.
    process: OccupiedEntry<'a, Pid, Process<TPud, TTud>, DefaultHashBuilder>,

    /// Index of the thread within the [`vm::ProcessStateMachine`].
    thread_index: usize,
}

/// Outcome of the [`run`](ProcessesCollection::run) function.
// TODO: Debug
pub enum RunOneOutcome<'a, TPud, TTud> {
    /// The main thread of a process has finished.
    ///
    /// The process no longer exists.
    ProcessFinished {
        /// Pid of the process that has finished.
        pid: Pid,

        /// User data of the process.
        user_data: TPud,

        // TODO: return all the threads user data
        /// Value returned by the main thread that has finished.
        value: Option<wasmi::RuntimeValue>,
    },

    /// A thread in a process has finished.
    ThreadFinished {
        /// Process whose thread has finished.
        process: ProcessesCollectionProc<'a, TPud, TTud>,

        /// User data of the thread.
        user_data: TTud,

        /// Value returned by the function that was executed.
        value: Option<wasmi::RuntimeValue>,
    },

    /// The currently-executed function has been paused due to a call to an external function.
    ///
    /// This variant contains the identifier of the external function that is expected to be
    /// called, and its parameters. When you call [`resume`](ProcessesCollectionProc::resume) again,
    /// you must pass back the outcome of calling that function.
    Interrupted {
        /// Thread that has been interrupted.
        thread: ProcessesCollectionThread<'a, TPud, TTud>,

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
        user_data: TPud,
        // TODO: return all the threads user data
        /// Error that happened.
        // TODO: error type should change here
        error: wasmi::Trap,
    },

    /// No thread is ready to run. Nothing was done.
    Idle,
}

/// Minimum capacity of the container of the list of processes.
///
/// If we shrink the container too much, then it will have to perform lots of allocations in order
/// to grow again in the future. We therefore avoid that situation.
const PROCESSES_MIN_CAPACITY: usize = 128;

impl<TPud, TTud> ProcessesCollection<TPud, TTud> {
    pub fn new() -> Self {
        ProcessesCollection {
            pid_pool: PidPool::new(),
            next_thread_id: ThreadId(1),
            processes: HashMap::with_capacity(PROCESSES_MIN_CAPACITY),
        }
    }

    pub fn execute(
        &mut self,
        module: &Module,
        proc_user_data: TPud,
        main_thread_user_data: TTud,
        symbols: impl FnMut(&InterfaceId, &str, &wasmi::Signature) -> Result<usize, ()>,
    ) -> Result<ProcessesCollectionProc<TPud, TTud>, vm::NewErr> {
        let main_thread_id = {
            let id = self.next_thread_id;
            self.next_thread_id.0 += 1;
            id
        };

        let main_thread_data = Thread {
            user_data: main_thread_user_data,
            thread_id: main_thread_id,
            value_back: Some(None),
        };

        let state_machine = vm::ProcessStateMachine::new(module, main_thread_data, symbols)?;

        // We only modify `self` at the very end.
        let new_pid = self.pid_pool.assign();
        self.processes.insert(
            new_pid,
            Process {
                state_machine,
                user_data: proc_user_data,
            },
        );
        // Shrink the list from time to time so that it doesn't grow too much.
        if u64::from(new_pid) % 256 == 0 {
            self.processes.shrink_to(PROCESSES_MIN_CAPACITY);
        }
        Ok(self.process_by_id(new_pid).unwrap())
    }

    /// Runs one thread amongst the collection.
    pub fn run(&mut self) -> RunOneOutcome<TPud, TTud> {
        // We start by finding a thread in `self.processes` that is ready to run.
        let (mut process, thread_index): (OccupiedEntry<_, _, _>, usize) = {
            let entry = self
                .processes
                .iter_mut()
                .filter_map(|(k, p)| {
                    if let Some(i) = p.ready_to_run_thread_index() {
                        Some((*k, i))
                    } else {
                        None
                    }
                })
                .next();
            match entry {
                Some((pid, thread_index)) => match self.processes.entry(pid) {
                    Entry::Occupied(p) => (p, thread_index),
                    Entry::Vacant(_) => unreachable!(),
                },
                None => return RunOneOutcome::Idle,
            }
        };

        let run_outcome = {
            let mut thread = process
                .get_mut()
                .state_machine
                .thread(thread_index)
                .unwrap();
            let value_back = thread.user_data().value_back.take().unwrap();
            thread.run(value_back)
        };

        match run_outcome {
            Err(vm::RunErr::BadValueTy { .. }) => panic!(), // TODO:
            Err(vm::RunErr::Poisoned) => unreachable!(),
            Ok(vm::ExecOutcome::ThreadFinished {
                thread_index: 0,
                return_value,
                ..
            }) => {
                let (pid, Process { user_data, .. }) = process.remove_entry();
                RunOneOutcome::ProcessFinished {
                    pid,
                    user_data,
                    value: return_value,
                }
            }
            Ok(vm::ExecOutcome::ThreadFinished {
                return_value,
                user_data,
                ..
            }) => RunOneOutcome::ThreadFinished {
                process: ProcessesCollectionProc { process },
                user_data: user_data.user_data,
                value: return_value,
            },
            Ok(vm::ExecOutcome::Interrupted { id, params, .. }) => RunOneOutcome::Interrupted {
                thread: ProcessesCollectionThread {
                    process,
                    thread_index,
                },
                id,
                params,
            },
            Ok(vm::ExecOutcome::Errored { error, .. }) => {
                let (pid, Process { user_data, .. }) = process.remove_entry();
                RunOneOutcome::Errored {
                    pid,
                    user_data,
                    error,
                }
            }
        }
    }

    /// Returns an iterator to all the processes that exist in the collection.
    pub fn pids<'a>(&'a self) -> impl ExactSizeIterator<Item = Pid> + 'a {
        self.processes.keys().cloned()
    }

    /// Returns a process by its [`Pid`], if it exists.
    pub fn process_by_id(&mut self, pid: Pid) -> Option<ProcessesCollectionProc<TPud, TTud>> {
        match self.processes.entry(pid) {
            Entry::Occupied(e) => Some(ProcessesCollectionProc { process: e }),
            Entry::Vacant(_) => None,
        }
    }
}

impl<TPud, TTud> Default for ProcessesCollection<TPud, TTud> {
    fn default() -> Self {
        Self::new()
    }
}

impl<TPud, TTud> Process<TPud, TTud> {
    /// Finds a thread in this process that is ready to be executed.
    fn ready_to_run_thread_index(&mut self) -> Option<usize> {
        for thread_n in 0..self.state_machine.num_threads() {
            let mut thread = self.state_machine.thread(thread_n).unwrap();
            if thread.user_data().value_back.is_some() {
                return Some(thread_n);
            }
        }

        None
    }
}

impl<'a, TPud, TTud> ProcessesCollectionProc<'a, TPud, TTud> {
    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollection::process_by_id).
    pub fn pid(&self) -> Pid {
        *self.process.key()
    }

    /// Returns the user data that is associated to the process.
    pub fn user_data(&mut self) -> &mut TPud {
        &mut self.process.get_mut().user_data
    }

    /// Adds a new thread to the process, starting the function with the given index and passing
    /// the given parameters.
    // TODO: return Result
    // TODO: don't expose wasmi::RuntimeValue
    pub fn start_thread(
        &mut self,
        fn_index: u32,
        params: Vec<wasmi::RuntimeValue>,
        user_data: TTud,
    ) {
        let thread_id = ThreadId(5555); /* FIXME: {
                                            let id = self.next_thread_id;
                                            self.next_thread_id.0 += 1;
                                            id
                                        };*/

        let thread_data = Thread {
            user_data,
            thread_id,
            value_back: Some(None),
        };

        self.process
            .get_mut()
            .state_machine
            .start_thread_by_id(fn_index, params, thread_data);
    }

    pub fn main_thread(self) -> ProcessesCollectionThread<'a, TPud, TTud> {
        ProcessesCollectionThread {
            process: self.process,
            thread_index: 0,
        }
    }

    // TODO: adjust to final API
    pub fn read_memory(&mut self, range: impl RangeBounds<usize>) -> Result<Vec<u8>, ()> {
        self.process.get_mut().state_machine.read_memory(range)
    }

    /// Write the data at the given memory location.
    ///
    /// Returns an error if the range is invalid or out of range.
    pub fn write_memory(&mut self, offset: u32, value: &[u8]) -> Result<(), ()> {
        self.process
            .get_mut()
            .state_machine
            .write_memory(offset, value)
    }

    /// Aborts the process and returns the associated user data.
    pub fn abort(self) -> TPud {
        let (_, Process { user_data, .. }) = self.process.remove_entry();
        user_data
    }
}

impl<'a, TPud, TTud> ProcessesCollectionThread<'a, TPud, TTud> {
    fn inner(&mut self) -> vm::Thread<Thread<TTud>> {
        self.process
            .get_mut()
            .state_machine
            .thread(self.thread_index)
            .unwrap()
    }

    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollection::process_by_id).
    pub fn pid(&self) -> Pid {
        *self.process.key()
    }

    pub fn next_thread(mut self) -> Option<ProcessesCollectionThread<'a, TPud, TTud>> {
        self.thread_index += 1;
        if self.thread_index >= self.process.get_mut().state_machine.num_threads() {
            return None;
        }

        Some(self)
    }

    /// Returns the user data that is associated to the process.
    pub fn process_user_data(&mut self) -> &mut TPud {
        &mut self.process.get_mut().user_data
    }

    /// Returns the user data that is associated to the thread.
    pub fn user_data(&mut self) -> &mut TTud {
        &mut self.inner().into_user_data().user_data
    }

    /// After [`RunOneOutcome::Interrupted`] is returned, use this function to feed back the value
    /// to use as the return type of the function that has been called.
    pub fn resume(&mut self, value: Option<wasmi::RuntimeValue>) {
        let user_data = self.inner().into_user_data();

        // TODO: check type of the value?
        if user_data.value_back.is_some() {
            panic!()
        }

        user_data.value_back = Some(value);
    }

    // TODO: adjust to final API
    pub fn read_memory(&mut self, range: impl RangeBounds<usize>) -> Result<Vec<u8>, ()> {
        self.process.get_mut().state_machine.read_memory(range)
    }

    /// Write the data at the given memory location.
    ///
    /// Returns an error if the range is invalid or out of range.
    pub fn write_memory(&mut self, offset: u32, value: &[u8]) -> Result<(), ()> {
        self.process
            .get_mut()
            .state_machine
            .write_memory(offset, value)
    }
}
