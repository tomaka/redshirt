// Copyright (C) 2019  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::id_pool::IdPool;
use crate::module::Module;
use crate::scheduler::{vm, Pid};
use crate::signature::Signature;
use alloc::{borrow::Cow, vec::Vec};
use core::fmt;
use hashbrown::{
    hash_map::{DefaultHashBuilder, Entry, OccupiedEntry},
    HashMap,
};
use rand::seq::SliceRandom as _;

/// Collection of multiple [`ProcessStateMachine`](vm::ProcessStateMachine)s grouped together in a
/// smart way.
///
/// This struct handles interleaving processes execution.
///
/// The generic parameters `TPud` and `TTud` are "user data"s that are stored per process and per
/// thread, and allows the user to put extra information associated to a process or a thread.
pub struct ProcessesCollection<TExtr, TPud, TTud> {
    /// Allocations of process IDs.
    pid_pool: IdPool,

    /// Allocation of thread IDs.
    tid_pool: IdPool,

    /// List of running processes.
    processes: HashMap<Pid, Process<TPud, TTud>>,

    /// List of functions that processes can call.
    /// The key of this map is an arbitrary `usize` that we pass to the WASM interpreter.
    /// This field is never modified after the `ProcessesCollection` is created.
    extrinsics: HashMap<usize, TExtr>,

    /// Map used to resolve imports when starting a process.
    /// For each module and function name, stores the signature and an arbitrary usize that
    /// corresponds to the entry in `extrinsics`.
    /// This field is never modified after the `Core` is created.
    extrinsics_id_assign: HashMap<(Cow<'static, str>, Cow<'static, str>), (usize, Signature)>,
}

/// Prototype for a `ProcessesCollection` under construction.
pub struct ProcessesCollectionBuilder<TExtr> {
    /// See the corresponding field in `ProcessesCollection`.
    extrinsics: HashMap<usize, TExtr>,
    /// See the corresponding field in `ProcessesCollection`.
    extrinsics_id_assign: HashMap<(Cow<'static, str>, Cow<'static, str>), (usize, Signature)>,
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

    /// Reference to the same field in [`ProcessesCollection`].
    tid_pool: &'a mut IdPool,
}

/// Access to a thread within the collection.
pub struct ProcessesCollectionThread<'a, TPud, TTud> {
    /// Pointer within the hashmap.
    process: OccupiedEntry<'a, Pid, Process<TPud, TTud>, DefaultHashBuilder>,

    /// Index of the thread within the [`vm::ProcessStateMachine`].
    thread_index: usize,
}

/// Outcome of the [`run`](ProcessesCollection::run) function.
#[derive(Debug)]
pub enum RunOneOutcome<'a, TExtr, TPud, TTud> {
    /// Either the main thread of a process has finished, or a fatal error was encountered.
    ///
    /// The process no longer exists.
    ProcessFinished {
        /// Pid of the process that has finished.
        pid: Pid,

        /// User data of the process.
        user_data: TPud,

        /// Id and user datas of all the threads of the process. The first element is the main
        /// thread's.
        /// These threads no longer exist.
        dead_threads: Vec<(ThreadId, TTud)>,

        /// Value returned by the main thread that has finished, or error that happened.
        outcome: Result<Option<wasmi::RuntimeValue>, wasmi::Trap>,
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
    /// called, and its parameters. When you call [`resume`](ProcessesCollectionThread::resume)
    /// again, you must pass back the outcome of calling that function.
    Interrupted {
        /// Thread that has been interrupted.
        thread: ProcessesCollectionThread<'a, TPud, TTud>,

        /// Identifier of the function to call. Corresponds to the value provided at
        /// initialization when resolving imports.
        id: &'a mut TExtr,

        /// Parameters of the function call.
        params: Vec<wasmi::RuntimeValue>,
    },

    /// No thread is ready to run. Nothing was done.
    Idle,
}

/// Minimum capacity of the container of the list of processes.
///
/// If we shrink the container too much, then it will have to perform lots of allocations in order
/// to grow again in the future. We therefore avoid that situation.
const PROCESSES_MIN_CAPACITY: usize = 128;

impl<TExtr, TPud, TTud> ProcessesCollection<TExtr, TPud, TTud> {
    /// Creates a new process state machine from the given module.
    ///
    /// The closure is called for each import that the module has. It must assign a number to each
    /// import, or return an error if the import can't be resolved. When the VM calls one of these
    /// functions, this number will be returned back in order for the user to know how to handle
    /// the call.
    ///
    /// A single main thread (whose user data is passed by parameter) is automatically created and
    /// is paused at the start of the "_start" function of the module.
    pub fn execute(
        &mut self,
        module: &Module,
        proc_user_data: TPud,
        main_thread_user_data: TTud,
    ) -> Result<ProcessesCollectionProc<TPud, TTud>, vm::NewErr> {
        let main_thread_id = self.tid_pool.assign(); // TODO: check for duplicates
        let main_thread_data = Thread {
            user_data: main_thread_user_data,
            thread_id: main_thread_id,
            value_back: Some(None),
        };

        let state_machine = {
            let extrinsics_id_assign = &mut self.extrinsics_id_assign;
            vm::ProcessStateMachine::new(
                module,
                main_thread_data,
                move |interface, function, obtained_signature| {
                    if let Some((index, expected_signature)) =
                        extrinsics_id_assign.get(&(interface.into(), function.into()))
                    {
                        if expected_signature.matches_wasmi(obtained_signature) {
                            return Ok(*index);
                        } else {
                            // TODO: way to report the signature mismatch?
                        }
                    }

                    Err(())
                },
            )?
        };

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
    ///
    /// Which thread is run is implementation-defined and no guarantee is made.
    pub fn run(&mut self) -> RunOneOutcome<TExtr, TPud, TTud> {
        // We start by finding a thread in `self.processes` that is ready to run.
        let (mut process, thread_index): (OccupiedEntry<_, _, _>, usize) = {
            let mut entries = self.processes.iter_mut().collect::<Vec<_>>();
            entries.shuffle(&mut rand::thread_rng());
            let entry = entries
                .into_iter()
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
                user_data: main_thread_user_data,
            }) => {
                let (
                    pid,
                    Process {
                        user_data,
                        state_machine,
                    },
                ) = process.remove_entry();
                let other_threads_ud = state_machine.into_user_datas();
                let mut dead_threads = Vec::with_capacity(1 + other_threads_ud.len());
                dead_threads.push((
                    main_thread_user_data.thread_id,
                    main_thread_user_data.user_data,
                ));
                for thread in other_threads_ud {
                    dead_threads.push((thread.thread_id, thread.user_data));
                }
                debug_assert_eq!(dead_threads.len(), dead_threads.capacity());
                RunOneOutcome::ProcessFinished {
                    pid,
                    user_data,
                    dead_threads,
                    outcome: Ok(return_value),
                }
            }
            Ok(vm::ExecOutcome::ThreadFinished {
                return_value,
                user_data,
                ..
            }) => RunOneOutcome::ThreadFinished {
                process: ProcessesCollectionProc {
                    process,
                    tid_pool: &mut self.tid_pool,
                },
                user_data: user_data.user_data,
                value: return_value,
            },
            Ok(vm::ExecOutcome::Interrupted { id, params, .. }) => {
                // TODO: check params against signature with a debug_assert
                let extrinsic = self.extrinsics.get_mut(&id).unwrap();
                RunOneOutcome::Interrupted {
                    thread: ProcessesCollectionThread {
                        process,
                        thread_index,
                    },
                    id: extrinsic,
                    params,
                }
            }
            Ok(vm::ExecOutcome::Errored { error, .. }) => {
                let (
                    pid,
                    Process {
                        user_data,
                        state_machine,
                    },
                ) = process.remove_entry();
                let dead_threads = state_machine
                    .into_user_datas()
                    .map(|t| (t.thread_id, t.user_data))
                    .collect::<Vec<_>>();
                RunOneOutcome::ProcessFinished {
                    pid,
                    user_data,
                    dead_threads,
                    outcome: Err(error),
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
            Entry::Occupied(e) => Some(ProcessesCollectionProc {
                process: e,
                tid_pool: &mut self.tid_pool,
            }),
            Entry::Vacant(_) => None,
        }
    }

    /// Returns a thread by its [`ThreadId`], if it exists.
    pub fn thread_by_id(&mut self, id: ThreadId) -> Option<ProcessesCollectionThread<TPud, TTud>> {
        // TODO: ouch that's O(n)

        let mut loop_out = None;
        for (pid, process) in self.processes.iter_mut() {
            for thread_index in 0..process.state_machine.num_threads() {
                let mut thread = process.state_machine.thread(thread_index).unwrap();
                if thread.user_data().thread_id == id {
                    loop_out = Some((pid.clone(), thread_index));
                    break;
                }
            }
        }

        let (pid, thread_index) = loop_out?;
        Some(ProcessesCollectionThread {
            process: match self.processes.entry(pid) {
                Entry::Vacant(_) => unreachable!(),
                Entry::Occupied(e) => e,
            },
            thread_index,
        })
    }
}

impl<TExtr> Default for ProcessesCollectionBuilder<TExtr> {
    fn default() -> ProcessesCollectionBuilder<TExtr> {
        ProcessesCollectionBuilder {
            extrinsics: Default::default(),
            extrinsics_id_assign: Default::default(),
        }
    }
}

impl<TExtr> ProcessesCollectionBuilder<TExtr> {
    /// Registers a function that is available for processes to call.
    ///
    /// The function is registered under the given interface and function name. If a WASM module
    /// imports a function with the corresponding interface and function name combination and
    /// calls it, a [`RunOneOutcome::Interrupted`] event will be generated, containing the token
    /// passed as parameter.
    ///
    /// The function signature passed as parameter is enforced when the process is created.
    ///
    /// # Panic
    ///
    /// Panics if an extrinsic with this interface/name combination has already been registered.
    ///
    pub fn with_extrinsic(
        mut self,
        interface: impl Into<Cow<'static, str>>,
        f_name: impl Into<Cow<'static, str>>,
        signature: Signature,
        token: impl Into<TExtr>,
    ) -> Self {
        let interface = interface.into();
        let f_name = f_name.into();

        let index = self.extrinsics.len();
        debug_assert!(!self.extrinsics.contains_key(&index));
        match self.extrinsics_id_assign.entry((interface, f_name)) {
            Entry::Occupied(_) => panic!(),
            Entry::Vacant(e) => e.insert((index, signature)),
        };
        self.extrinsics.insert(index, token.into());
        self
    }

    /// Turns the builder into a [`ProcessesCollection`].
    pub fn build<TPud, TTud>(mut self) -> ProcessesCollection<TExtr, TPud, TTud> {
        // We're not going to modify these fields ever again, so let's free some memory.
        self.extrinsics.shrink_to_fit();
        self.extrinsics_id_assign.shrink_to_fit();
        debug_assert_eq!(self.extrinsics.len(), self.extrinsics_id_assign.len());

        ProcessesCollection {
            pid_pool: IdPool::new(),
            tid_pool: IdPool::new(),
            processes: HashMap::with_capacity(PROCESSES_MIN_CAPACITY),
            extrinsics: self.extrinsics,
            extrinsics_id_assign: self.extrinsics_id_assign,
        }
    }
}

impl From<u64> for ThreadId {
    fn from(id: u64) -> ThreadId {
        ThreadId(id)
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
    // TODO: don't expose wasmi::RuntimeValue in the API
    pub fn start_thread(
        mut self,
        fn_index: u32,
        params: Vec<wasmi::RuntimeValue>,
        user_data: TTud,
    ) -> Result<ProcessesCollectionThread<'a, TPud, TTud>, vm::StartErr> {
        let thread_id = self.tid_pool.assign(); // TODO: check for duplicates
        let thread_data = Thread {
            user_data,
            thread_id,
            value_back: Some(None),
        };

        self.process
            .get_mut()
            .state_machine
            .start_thread_by_id(fn_index, params, thread_data)?;

        let thread_index = self.process.get_mut().state_machine.num_threads();
        Ok(ProcessesCollectionThread {
            process: self.process,
            thread_index,
        })
    }

    /// Returns an object representing the main thread of this process.
    ///
    /// The "main thread" of a process is created automatically when you call
    /// [`ProcessesCollection::execute`]. If it stops, the entire process stops.
    pub fn main_thread(self) -> ProcessesCollectionThread<'a, TPud, TTud> {
        ProcessesCollectionThread {
            process: self.process,
            thread_index: 0,
        }
    }

    pub fn read_memory(&mut self, offset: u32, size: u32) -> Result<Vec<u8>, ()> {
        self.process
            .get_mut()
            .state_machine
            .read_memory(offset, size)
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
        // TODO: return thread user datas as well
        let (_, Process { user_data, .. }) = self.process.remove_entry();
        user_data
    }
}

impl<'a, TPud, TTud> fmt::Debug for ProcessesCollectionProc<'a, TPud, TTud>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: threads user data
        f.debug_struct("ProcessesCollectionProc")
            .field("pid", &self.pid())
            //.field("user_data", self.user_data())     // TODO: requires &mut self :-/
            .finish()
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

    /// Returns the id of the thread. Allows later retrieval by calling
    /// [`thread_by_id`](ProcessesCollection::thread_by_id).
    ///
    /// [`ThreadId`]s are unique within a [`ProcessesCollection`], independently from the process.
    pub fn tid(&mut self) -> ThreadId {
        self.inner().into_user_data().thread_id
    }

    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollection::process_by_id).
    pub fn pid(&self) -> Pid {
        *self.process.key()
    }

    /// Returns the following thread within the next process, or `None` if this is the last thread.
    ///
    /// Threads are ordered arbitrarily. In particular, they are **not** ordered by [`ThreadId`].
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

    pub fn read_memory(&mut self, offset: u32, size: u32) -> Result<Vec<u8>, ()> {
        self.process
            .get_mut()
            .state_machine
            .read_memory(offset, size)
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

impl<'a, TPud, TTud> fmt::Debug for ProcessesCollectionThread<'a, TPud, TTud>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        //let id = self.id();
        let pid = self.pid();
        // TODO: requires &mut self :-/
        //let ready_to_run = self.inner().into_user_data().value_back.is_some();

        f.debug_struct("ProcessesCollectionThread")
            .field("pid", &pid)
            //.field("thread_id", &id)
            //.field("user_data", self.user_data())
            //.field("ready_to_run", &ready_to_run)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessesCollectionBuilder;
    use crate::sig;

    #[test]
    #[should_panic]
    fn panic_duplicate_extrinsic() {
        ProcessesCollectionBuilder::<()>::default()
            .with_extrinsic("foo", "test", sig!(()), ())
            .with_extrinsic("foo", "test", sig!(()), ());
    }
}
