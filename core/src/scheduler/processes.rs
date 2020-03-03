// Copyright (C) 2019-2020  Pierre Krieger
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

use crate::{Pid, ThreadId};
use crate::id_pool::IdPool;
use crate::module::Module;
use crate::scheduler::vm;
use crate::signature::Signature;

use alloc::{borrow::Cow, sync::Arc, vec::Vec};
use core::{cell::RefCell, fmt};
use fnv::FnvBuildHasher;
use hashbrown::{
    hash_map::{Entry, OccupiedEntry},
    HashMap,
};
use nohash_hasher::BuildNoHashHasher;
use spin::Mutex;

/// Collection of multiple [`ProcessStateMachine`](vm::ProcessStateMachine)s grouped together in a
/// smart way.
///
/// This struct handles interleaving processes execution.
///
/// The generic parameters `TPud` and `TTud` are "user data"s that are stored respectively per
/// process and per thread, and allows the user to put extra information associated to a process
/// or a thread.
pub struct ProcessesCollection<TExtr, TPud, TTud> {
    /// Allocations of process IDs.
    pid_pool: IdPool,

    /// Allocation of thread IDs.
    // TODO: merge with pid_pool?
    tid_pool: IdPool,

    /// List of running processes.
    ///
    /// We hold `Arc`s to processes in order to count the number of "locks" that have been
    /// acquired. Entries are removed from this list only if the number of strong counts is
    /// equal to 1.
    processes: RefCell<HashMap<Pid, Arc<Process<TPud, TTud>>, BuildNoHashHasher<u64>>>,

    /// List of functions that processes can call.
    /// The key of this map is an arbitrary `usize` that we pass to the WASM interpreter.
    /// This field is never modified after the [`ProcessesCollection`] is created.
    extrinsics: Vec<TExtr>,

    /// Map used to resolve imports when starting a process.
    /// For each module and function name, stores the signature and an arbitrary usize that
    /// corresponds to the entry in `extrinsics`.
    /// This field is never modified after the [`ProcessesCollection`] is created.
    extrinsics_id_assign:
        HashMap<(Cow<'static, str>, Cow<'static, str>), (usize, Signature), FnvBuildHasher>,
}

/// Prototype for a `ProcessesCollection` under construction.
pub struct ProcessesCollectionBuilder<TExtr> {
    /// See the corresponding field in `ProcessesCollection`.
    pid_pool: IdPool,
    /// See the corresponding field in `ProcessesCollection`.
    extrinsics: Vec<TExtr>,
    /// See the corresponding field in `ProcessesCollection`.
    extrinsics_id_assign:
        HashMap<(Cow<'static, str>, Cow<'static, str>), (usize, Signature), FnvBuildHasher>,
}

struct Process<TPud, TTud> {
    /// Identifier of the process.
    pid: Pid,

    /// The virtual machine.
    // TODO: can we talk about this mutex? it's really not great to have a mutex
    state_machine: Mutex<vm::ProcessStateMachine<Thread<TTud>>>,

    /// User-chosen data (opaque to us) that describes the process.
    user_data: TPud,
}

/// Additional data associated to a thread. Stored within the [`vm::ProcessStateMachine`].
struct Thread<TTud> {
    /// User-chosen data (opaque to us) that describes the thread.
    /// Contains `None` if and only if the thread is "locked" by the user.
    /// By logic, can never contain `None` if `value_back` is `Some`.
    user_data: Option<TTud>,

    /// Identifier of the thread.
    thread_id: ThreadId,

    /// Value to use when resuming. If `Some`, the process is ready for a round of running. If
    /// `None`, then we're waiting for the user to call `resume`.
    value_back: Option<Option<wasmi::RuntimeValue>>,
}

/// Access to a process within the collection.
pub struct ProcessesCollectionProc<'a, TPud, TTud> {
    process: Arc<Process<TPud, TTud>>,

    /// Reference to the same field in [`ProcessesCollection`].
    tid_pool: &'a IdPool,
}

/// Access to a thread within the collection.
pub struct ProcessesCollectionThread<'a, TPud, TTud> {
    process: Arc<Process<TPud, TTud>>,

    /// Identifier of the thread. Must always match one of the user data in the virtual machine.
    tid: ThreadId,

    /// Reference to the same field in [`ProcessesCollection`].
    tid_pool: &'a IdPool,

    /// User data extracted from [`Thread`]. Must be put back when this struct is destroyed.
    /// Always `Some`, except right before destruction.
    user_data: Option<TTud>,
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
        /// Thread which has finished.
        thread_id: ThreadId,

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
        &self,
        module: &Module,
        proc_user_data: TPud,
        main_thread_user_data: TTud,
    ) -> Result<ProcessesCollectionProc<TPud, TTud>, vm::NewErr> {
        let main_thread_id = self.tid_pool.assign(); // TODO: check for duplicates
        let main_thread_data = Thread {
            user_data: Some(main_thread_user_data),
            thread_id: main_thread_id,
            value_back: Some(None),
        };

        let state_machine = {
            let extrinsics_id_assign = &self.extrinsics_id_assign;
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
        let process = Arc::new(Process {
            pid: new_pid,
            state_machine: Mutex::new(state_machine),
            user_data: proc_user_data,
        });
        let processes = self.processes.borrow_mut();
        processes.insert(
            new_pid,
            process.clone(),
        );

        // Shrink the list from time to time so that it doesn't grow too much.
        if u64::from(new_pid) % 256 == 0 {
            processes.shrink_to(PROCESSES_MIN_CAPACITY);
        }

        Ok(ProcessesCollectionProc {
            process,
            tid_pool: &self.tid_pool,
        })
    }

    /// Runs one thread amongst the collection.
    ///
    /// Which thread is run is implementation-defined and no guarantee is made.
    pub fn run(&self) -> RunOneOutcome<TExtr, TPud, TTud> {
        let processes = self.processes.borrow_mut();

        // We start by finding a thread in `self.processes` that is ready to run.
        let (mut process, inner_thread_index): (OccupiedEntry<_, _, _>, usize) = {
            let entries = processes.iter_mut().collect::<Vec<_>>();
            // TODO: entries.shuffle(&mut rand::thread_rng());
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
                Some((pid, inner_thread_index)) => match processes.entry(pid) {
                    Entry::Occupied(p) => (p, inner_thread_index),
                    Entry::Vacant(_) => unreachable!(),
                },
                None => return RunOneOutcome::Idle,
            }
        };

        // Now run the thread until something happens.
        let run_outcome = {
            // TODO: keeps the mutex locked while running, obviously not great
            let mut thread = match process.get_mut().state_machine.lock().thread(inner_thread_index) {
                Some(t) => t,
                None => unreachable!(),
            };
            let value_back = match thread.user_data().value_back.take() {
                Some(vb) => vb,
                None => unreachable!(),
            };
            thread.run(value_back)
        };

        match run_outcome {
            Err(vm::RunErr::BadValueTy { .. }) => panic!(), // TODO:
            Err(vm::RunErr::Poisoned) => unreachable!(),

            // A process has ended.
            Ok(vm::ExecOutcome::ThreadFinished {
                thread_index: 0,
                return_value,
                user_data: main_thread_user_data,
            }) => {
                let (pid, proc) = process.remove_entry();
                let other_threads_ud = proc.state_machine.lock().into_user_datas();
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
                    user_data: proc.user_data,
                    dead_threads,
                    outcome: Ok(return_value),
                }
            }

            // A thread has ended.
            Ok(vm::ExecOutcome::ThreadFinished {
                return_value,
                user_data,
                ..
            }) => RunOneOutcome::ThreadFinished {
                thread_id: user_data.thread_id,
                process: ProcessesCollectionProc {
                    process,
                    tid_pool: &mut self.tid_pool,
                },
                user_data: match user_data.user_data.take() {
                    Some(ud) => ud,
                    None => unreachable!(),
                },
                value: return_value,
            },

            // Thread wants to call an extrinsic function.
            Ok(vm::ExecOutcome::Interrupted { thread, id, params }) => {
                // TODO: check params against signature with a debug_assert
                let extrinsic = match self.extrinsics.get_mut(id) {
                    Some(e) => e,
                    None => unreachable!(),
                };
                let user_data = match thread.user_data().user_data.take() {
                    Some(ud) => ud,
                    None => unreachable!(),
                };
                RunOneOutcome::Interrupted {
                    thread: ProcessesCollectionThread {
                        process,
                        thread_index: inner_thread_index,
                        tid_pool: &self.tid_pool,
                        user_data,
                    },
                    id: extrinsic,
                    params,
                }
            }

            // An error happened during the execution. We kill the entire process.
            Ok(vm::ExecOutcome::Errored { error, .. }) => {
                let (pid, proc) = process.remove_entry();
                let dead_threads = proc
                    .state_machine
                    .into_user_datas()
                    .map(|t| (t.thread_id, t.user_data))
                    .collect::<Vec<_>>();
                RunOneOutcome::ProcessFinished {
                    pid,
                    user_data: proc.user_data,
                    dead_threads,
                    outcome: Err(error),
                }
            }
        }
    }

    /// Returns an iterator to all the processes that exist in the collection.
    ///
    /// This is equivalent to calling [`ProcessesCollection::process_by_id`] for each possible
    /// ID.
    pub fn pids<'a>(&'a self) -> impl ExactSizeIterator<Item = ProcessesCollectionProc<'a, TPud, TTud>> + 'a {
        let processes = self.processes.borrow();

        processes
            .values()
            .cloned()
            .map(|process| ProcessesCollectionProc {
                process,
                tid_pool: &self.tid_pool,
            })
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// Returns a process by its [`Pid`], if it exists.
    ///
    /// This function returns a "lock".
    /// While the lock is held, it isn't possible for a [`RunOneOutcome::ProcessFinished`]
    /// message to be returned for the given process.
    ///
    /// You can call this function mutiple times in order to obtain the lock multiple times.
    pub fn process_by_id(&self, pid: Pid) -> Option<ProcessesCollectionProc<TPud, TTud>> {
        let processes = self.processes.borrow();

        let p = processes.get(&pid)?.clone();
        // TODO: check whether process is dead

        debug_assert_eq!(p.pid, pid);

        Some(ProcessesCollectionProc {
            process: p,
            tid_pool: &mut self.tid_pool,
        })
    }

    /// Returns a thread by its [`ThreadId`], if it exists and is not running.
    ///
    /// Only threads that are currently paused waiting to be resumed can be grabbed using this
    /// method.
    ///
    /// Additionally, the returned object holds an exclusive lock to this thread. Calling this
    /// method twice for the same thread will fail the second time.
    pub fn interrupted_thread_by_id(&self, id: ThreadId) -> Option<ProcessesCollectionThread<TPud, TTud>> {
        // TODO: ouch that's O(n)
        let processes = self.processes.borrow_mut();

        let mut loop_out = None;
        for (pid, process) in processes.iter() {
            let process_lock = process.state_machine.lock();
            for thread_index in 0..process_lock.num_threads() {
                let mut thread = match process_lock.thread(thread_index) {
                    Some(t) => t,
                    None => unreachable!(),
                };
                if thread.user_data().thread_id == id {
                    // If thread isn't interrupted, return `None`.
                    if thread.user_data().value_back.is_none() {
                        return None;
                    }

                    // If the user data is already extracted, then this thread is locked.
                    let user_data = match thread.user_data().user_data.take() {
                        Some(ud) => ud,
                        None => return None,
                    };

                    loop_out = Some((process.clone(), pid.clone(), user_data));
                    break;
                }
            }
        }

        let (process, pid, user_data) = loop_out?;
        Some(ProcessesCollectionThread {
            process,
            tid: id,
            tid_pool: &self.tid_pool,
            user_data: Some(user_data),
        })
    }
}

impl<TExtr> Default for ProcessesCollectionBuilder<TExtr> {
    fn default() -> ProcessesCollectionBuilder<TExtr> {
        ProcessesCollectionBuilder {
            pid_pool: IdPool::new(),
            extrinsics: Default::default(),
            extrinsics_id_assign: Default::default(),
        }
    }
}

impl<TExtr> ProcessesCollectionBuilder<TExtr> {
    /// Allocates a `Pid` that will not be used by any process.
    ///
    /// > **Note**: As of the writing of this comment, this feature is only ever used to allocate
    /// >           `Pid`s that last forever. There is therefore no corresponding "unreserve_pid"
    /// >           method that frees such an allocated `Pid`. If there is ever a need to free
    /// >           these `Pid`s, such a method should be added.
    pub fn reserve_pid(&mut self) -> Pid {
        // Note that we take `&mut self`. It could be `&self`, but that would expose
        // implementation details.
        self.pid_pool.assign()
    }

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
        match self.extrinsics_id_assign.entry((interface, f_name)) {
            Entry::Occupied(_) => panic!(),
            Entry::Vacant(e) => e.insert((index, signature)),
        };
        self.extrinsics.push(token.into());
        self
    }

    /// Turns the builder into a [`ProcessesCollection`].
    pub fn build<TPud, TTud>(mut self) -> ProcessesCollection<TExtr, TPud, TTud> {
        // We're not going to modify these fields ever again, so let's free some memory.
        self.extrinsics.shrink_to_fit();
        self.extrinsics_id_assign.shrink_to_fit();
        debug_assert_eq!(self.extrinsics.len(), self.extrinsics_id_assign.len());

        ProcessesCollection {
            pid_pool: self.pid_pool,
            tid_pool: IdPool::new(),
            processes: RefCell::new(HashMap::with_capacity_and_hasher(
                PROCESSES_MIN_CAPACITY,
                Default::default(),
            )),
            extrinsics: self.extrinsics,
            extrinsics_id_assign: self.extrinsics_id_assign,
        }
    }
}

impl<TPud, TTud> Process<TPud, TTud> {
    /// Finds a thread in this process that is ready to be executed.
    fn ready_to_run_thread_index(&mut self) -> Option<usize> {
        let lock = self.state_machine.lock();
        for thread_n in 0..lock.num_threads() {
            let mut thread = match lock.thread(thread_n) {
                Some(t) => t,
                None => unreachable!(),
            };
            if thread.user_data().value_back.is_some() {
                debug_assert!(thread.user_data().user_data.is_some());
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
        self.process.pid
    }

    /// Returns the user data that is associated to the process.
    pub fn user_data(&self) -> &TPud {
        &self.process.user_data
    }

    /// Adds a new thread to the process, starting the function with the given index and passing
    /// the given parameters.
    ///
    /// > **Note**: The "function ID" is the index of the function in the WASM module. WASM
    /// >           doesn't have function pointers. Instead, all the functions are part of a single
    /// >           global array of functions.
    // TODO: don't expose wasmi::RuntimeValue in the API
    pub fn start_thread(
        mut self,
        fn_index: u32,
        params: Vec<wasmi::RuntimeValue>,
        user_data: TTud,
    ) -> Result<ThreadId, vm::StartErr> {
        let thread_id = self.tid_pool.assign(); // TODO: check for duplicates
        let thread_data = Thread {
            user_data: Some(user_data),
            thread_id,
            value_back: Some(None),
        };

        self.process
            .state_machine
            .lock()
            .start_thread_by_id(fn_index, params, thread_data)?;

        Ok(thread_id)
    }

    // TODO: bad API because of unique lock system for threads
    pub fn interrupted_threads(&self) -> ProcessesCollectionThread<'a, TPud, TTud> {
        unimplemented!()
    }

    pub fn read_memory(&self, offset: u32, size: u32) -> Result<Vec<u8>, ()> {
        let lock = self.process.state_machine.lock();
        lock.read_memory(offset, size)
    }

    /// Write the data at the given memory location.
    ///
    /// Returns an error if the range is invalid or out of range.
    pub fn write_memory(&self, offset: u32, value: &[u8]) -> Result<(), ()> {
        let lock = self.process.state_machine.lock();
        lock.write_memory(offset, value)
    }
}

impl<'a, TPud, TTud> fmt::Debug for ProcessesCollectionProc<'a, TPud, TTud>
where
    TPud: fmt::Debug,
    TTud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: threads user datas
        f.debug_struct("ProcessesCollectionProc")
            .field("pid", &self.pid())
            //.field("user_data", self.user_data())     // TODO: requires &mut self :-/
            .finish()
    }
}

impl<'a, TPud, TTud> ProcessesCollectionThread<'a, TPud, TTud> {
    /// Returns the id of the thread. Allows later retrieval by calling
    /// [`thread_by_id`](ProcessesCollection::thread_by_id).
    ///
    /// [`ThreadId`]s are unique within a [`ProcessesCollection`], independently from the process.
    pub fn tid(&self) -> ThreadId {
        self.tid
    }

    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollection::process_by_id).
    pub fn pid(&self) -> Pid {
        self.process.pid
    }

    /// Returns the process this thread belongs to.
    pub fn process(&self) -> ProcessesCollectionProc<'a, TPud, TTud> {
        ProcessesCollectionProc {
            process: self.process.clone(),
            tid_pool: self.tid_pool,
        }
    }

    /// Returns the user data that is associated to the thread.
    pub fn user_data(&mut self) -> &mut TTud {
        self.user_data.as_mut().unwrap()
    }

    /// After [`RunOneOutcome::Interrupted`] is returned, use this function to feed back the value
    /// to use as the return type of the function that has been called.
    ///
    /// This releases the [`ProcessesCollectionThread`]. The thread can now potentially be run by
    /// calling [`ProcessesCollection::run`].
    pub fn resume(mut self, value: Option<wasmi::RuntimeValue>) {
        let lock = self.process.state_machine.lock();

        for thread_index in 0..lock.num_threads() {
            let user_data = lock.thread(thread_index).unwrap().user_data()
            if user_data.thread_id != self.tid {
                continue;
            }

            // TODO: check type of the value?
            debug_assert!(user_data.value_back.is_none());
            user_data.value_back = Some(value);
            return;
        }

        panic!();
    }
}

impl<'a, TPud, TTud> Drop for ProcessesCollectionThread<'a, TPud, TTud> {
    fn drop(&mut self) {
        let mut lock = self.process.state_machine.lock();

        for thread_index in 0..lock.num_threads() {
            let user_data = lock.thread(thread_index).unwrap().user_data();
            if user_data.thread_id == self.tid {
                debug_assert!(user_data.user_data.is_none());
                user_data.user_data = Some(self.user_data.take().unwrap());
                return;
            }
        }

        unreachable!()
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
