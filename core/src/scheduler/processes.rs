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

use crate::id_pool::IdPool;
use crate::module::Module;
use crate::scheduler::vm;
use crate::signature::Signature;
use crate::{Pid, ThreadId};

use alloc::{
    borrow::Cow,
    collections::VecDeque,
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::fmt;
use crossbeam_queue::SegQueue;
use fnv::FnvBuildHasher;
use hashbrown::{hash_map::Entry, HashMap};
use nohash_hasher::BuildNoHashHasher;
use spinning_top::Spinlock;

/// Collection of multiple [`ProcessStateMachine`](vm::ProcessStateMachine)s grouped together in a
/// smart way.
///
/// This struct handles interleaving processes execution.
///
/// The generic parameters `TPud` and `TTud` are "user data"s that are stored respectively per
/// process and per thread, and allows the user to put extra information associated to a process
/// or a thread.
pub struct ProcessesCollection<TExtr, TPud, TTud> {
    /// Allocations of process IDs and thread IDs.
    pid_tid_pool: IdPool,

    /// Queue of processes with at least one thread to run. Every time a thread starts or is
    /// resumed, its process gets pushed to the end of this queue. In other words, there isn't
    /// any unnecessary entry.
    // TODO: use something better than a naive round robin?
    execution_queue: SegQueue<Arc<Process<TPud, TTud>>>,

    /// List of running processes.
    ///
    /// We hold `Weak`s to processes rather than `Arc`s. Processes are kept alive by the execution
    /// queue and the interrupted threads, thereby guaranteeing that they are alive only if they
    /// can potentially continue running.
    // TODO: find a solution for that mutex?
    processes: Spinlock<HashMap<Pid, Weak<Process<TPud, TTud>>, BuildNoHashHasher<u64>>>,

    /// List of threads waiting to be resumed, plus the user data and the process they belong to.
    /// Doesn't contain threads that are ready to run and threads that have been locked by the
    /// user with [`ProcessesCollection::interrupted_thread_by_id`].
    // TODO: find a solution for that mutex?
    interrupted_threads:
        Spinlock<HashMap<ThreadId, (TTud, Arc<Process<TPud, TTud>>), BuildNoHashHasher<u64>>>,

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

    /// Queue of process deaths to report to the external API.
    death_reports: SegQueue<(
        Pid,
        TPud,
        Vec<(ThreadId, TTud)>,
        Result<Option<wasmi::RuntimeValue>, wasmi::Trap>,
    )>,
}

/// Prototype for a [`ProcessesCollection`] under construction.
pub struct ProcessesCollectionBuilder<TExtr> {
    /// See the corresponding field in `ProcessesCollection`.
    pid_tid_pool: IdPool,
    /// See the corresponding field in `ProcessesCollection`.
    extrinsics: Vec<TExtr>,
    /// See the corresponding field in `ProcessesCollection`.
    extrinsics_id_assign:
        HashMap<(Cow<'static, str>, Cow<'static, str>), (usize, Signature), FnvBuildHasher>,
}

/// Description of a process. Always addressed through an `Arc`.
///
/// Note that the process might be dead.
struct Process<TPud, TTud> {
    /// Identifier of the process.
    pid: Pid,

    /// Part of the state behind a mutex.
    // TODO: it's obviously not great to have a Mutex here; this should be refactored once the
    // `vm` module supports multithreading
    lock: Spinlock<ProcessLock<TTud>>,

    /// User-chosen data (opaque to us) that describes the process.
    user_data: TPud,
}

/// Part of a process's state behind a mutex.
struct ProcessLock<TTud> {
    /// The actual Wasm virtual machine.
    vm: vm::ProcessStateMachine<Thread>,

    /// Queue of threads that are ready to be resumed.
    threads_to_resume: VecDeque<(ThreadId, TTud, Option<wasmi::RuntimeValue>)>,

    /// If `Some`, then the process is in a dead state, the virtual machine is in a poisoned
    /// state, and we are in the process of collecting all the threads user datas into the
    /// [`ProcessDeadState`] before notifying the user.
    dead: Option<ProcessDeadState<TTud>>,
}

/// Additional optional state to a process if it's been marked for destruction.
struct ProcessDeadState<TTud> {
    /// List of dead thread that we will ultimately send to the user.
    dead_threads: Vec<(ThreadId, TTud)>,

    /// Why the process ended. Never modified once set.
    outcome: Result<Option<wasmi::RuntimeValue>, wasmi::Trap>,
}

/// Additional data associated to a thread. Stored within the [`vm::ProcessStateMachine`].
struct Thread {
    /// Identifier of the thread.
    thread_id: ThreadId,
}

/// Access to a process within the collection.
pub struct ProcessesCollectionProc<'a, TExtr, TPud, TTud> {
    collection: &'a ProcessesCollection<TExtr, TPud, TTud>,
    process: Option<Arc<Process<TPud, TTud>>>,

    /// Reference to the same field in [`ProcessesCollection`].
    pid_tid_pool: &'a IdPool,
}

/// Access to a thread within the collection.
pub struct ProcessesCollectionThread<'a, TExtr, TPud, TTud> {
    collection: &'a ProcessesCollection<TExtr, TPud, TTud>,
    process: Option<Arc<Process<TPud, TTud>>>,

    /// Identifier of the thread. Must always match one of the user data in the virtual machine.
    tid: ThreadId,

    /// Reference to the same field in [`ProcessesCollection`].
    pid_tid_pool: &'a IdPool,

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
        process: ProcessesCollectionProc<'a, TExtr, TPud, TTud>,

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
        thread: ProcessesCollectionThread<'a, TExtr, TPud, TTud>,

        /// Identifier of the function to call. Corresponds to the value provided at
        /// initialization when resolving imports.
        id: &'a TExtr,

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
    ) -> Result<ProcessesCollectionProc<TExtr, TPud, TTud>, vm::NewErr> {
        let main_thread_id = self.pid_tid_pool.assign(); // TODO: check for duplicates?

        let state_machine = {
            let extrinsics_id_assign = &self.extrinsics_id_assign;
            vm::ProcessStateMachine::new(
                module,
                Thread {
                    thread_id: main_thread_id,
                },
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
        let new_pid = self.pid_tid_pool.assign();
        let process = Arc::new(Process {
            pid: new_pid,
            lock: Spinlock::new(ProcessLock {
                vm: state_machine,
                threads_to_resume: {
                    let mut queue = VecDeque::new();
                    queue.push_back((main_thread_id, main_thread_user_data, None));
                    queue
                },
                dead: None,
            }),
            user_data: proc_user_data,
        });

        {
            let mut processes = self.processes.lock();
            processes.insert(new_pid, Arc::downgrade(&process));
            // Shrink the list from time to time so that it doesn't grow too much.
            if u64::from(new_pid) % 256 == 0 {
                processes.shrink_to(PROCESSES_MIN_CAPACITY);
            }
        }

        self.execution_queue.push(process.clone());

        Ok(ProcessesCollectionProc {
            collection: self,
            process: Some(process),
            pid_tid_pool: &self.pid_tid_pool,
        })
    }

    /// Runs one thread amongst the collection.
    ///
    /// Which thread is run is implementation-defined and no guarantee is made.
    pub fn run(&self) -> RunOneOutcome<TExtr, TPud, TTud> {
        // We track the number of times this `loop` is run and panic if it seems like we're in an
        // infinite loop.
        let mut infinite_loop_guard = 0;

        loop {
            infinite_loop_guard += 1;
            assert!(
                infinite_loop_guard < 1_000_000,
                "infinite loop detected in scheduler"
            );

            // Items are pushed on `death_reports` when a `Process` struct is destroyed.
            if let Ok((pid, user_data, dead_threads, outcome)) = self.death_reports.pop() {
                return RunOneOutcome::ProcessFinished {
                    pid,
                    user_data,
                    dead_threads,
                    outcome,
                };
            }

            // We start by finding a process that is ready to run and lock it by extracting the
            // state machine.
            let process = match self.execution_queue.pop() {
                Ok(p) => p,
                Err(_) => return RunOneOutcome::Idle,
            };

            // "Lock" the process's state machine for execution.
            let mut proc_state = match process.lock.try_lock() {
                Some(st) => st,
                // If the process is already locked, push it back at the end of the queue.
                // TODO: this can turn into a spin loop, no? should handle "nothing to do"
                // situations
                None => {
                    // TODO: don't clone, but Rust throws a borrow error if we don't clone
                    self.execution_queue.push(process.clone());
                    continue;
                }
            };

            // If the process was in `self.execution_queue`, then we are guaranteed that a
            // thread was ready.
            let (tid, thread_user_data, resume_value) =
                proc_state.threads_to_resume.pop_front().unwrap();

            // If the process is marked as dying, insert the thread in the dying state and see if
            // we can finalize the destruction.
            if let Some(proc_dead) = &mut proc_state.dead {
                proc_dead.dead_threads.push((tid, thread_user_data));
                debug_assert!(proc_state.vm.is_poisoned());
                drop(proc_state); // Drop the lock.
                self.try_report_process_death(process);
                continue;
            }

            // Now run a thread until something happens.
            // This takes most of the CPU time of this function.
            let run_outcome = {
                debug_assert!(!proc_state.vm.is_poisoned());
                let thread_index = (0..proc_state.vm.num_threads())
                    .find(|n| proc_state.vm.thread(*n).unwrap().user_data().thread_id == tid)
                    .unwrap();
                proc_state
                    .vm
                    .thread(thread_index)
                    .unwrap()
                    .run(resume_value)
            };

            match run_outcome {
                Err(vm::RunErr::BadValueTy { .. }) => panic!(), // TODO:
                Err(vm::RunErr::Poisoned) => unreachable!(),

                // The entire process has ended or has crashed.
                Ok(vm::ExecOutcome::ThreadFinished {
                    thread_index: 0,
                    return_value,
                    user_data: main_thread_user_data,
                }) => {
                    debug_assert!(proc_state.vm.is_poisoned());
                    debug_assert!(proc_state.dead.is_none());

                    // TODO: Vec::with_capacity?
                    let mut dead_threads =
                        vec![(main_thread_user_data.thread_id, thread_user_data)];

                    // TODO: possible deadlock?
                    let mut threads = self.interrupted_threads.lock();
                    // TODO: O(n) complexity
                    while let Some(tid) = threads
                        .iter()
                        .find(|(_, (_, p))| Arc::ptr_eq(&process, p))
                        .map(|(k, _)| *k)
                    {
                        let (user_data, _) = threads.remove(&tid).unwrap();
                        dead_threads.push((tid, user_data));
                    }

                    proc_state.dead = Some(ProcessDeadState {
                        dead_threads,
                        outcome: Ok(return_value),
                    });
                }

                // The thread has ended.
                Ok(vm::ExecOutcome::ThreadFinished {
                    return_value,
                    user_data,
                    ..
                }) => {
                    debug_assert!(Arc::strong_count(&process) >= 2);
                    drop(proc_state);
                    return RunOneOutcome::ThreadFinished {
                        thread_id: user_data.thread_id,
                        process: ProcessesCollectionProc {
                            collection: self,
                            process: Some(process),
                            pid_tid_pool: &self.pid_tid_pool,
                        },
                        user_data: thread_user_data,
                        value: return_value,
                    };
                }

                // Thread wants to call an extrinsic function.
                Ok(vm::ExecOutcome::Interrupted {
                    mut thread,
                    id,
                    params,
                }) => {
                    // TODO: check params against signature with a debug_assert
                    let extrinsic = match self.extrinsics.get(id) {
                        Some(e) => e,
                        None => unreachable!(),
                    };
                    let tid = thread.user_data().thread_id;
                    drop(proc_state);
                    return RunOneOutcome::Interrupted {
                        thread: ProcessesCollectionThread {
                            collection: self,
                            process: Some(process),
                            tid,
                            pid_tid_pool: &self.pid_tid_pool,
                            user_data: Some(thread_user_data),
                        },
                        id: extrinsic,
                        params,
                    };
                }

                // An error happened during the execution. We kill the entire process.
                Ok(vm::ExecOutcome::Errored { error, .. }) => {
                    unimplemented!() // TODO:
                }
            }

            // If we reach here, the process has to be terminated.
            drop(proc_state);
            self.try_report_process_death(process);
        }
    }

    /// Returns an iterator to all the processes that exist in the collection.
    ///
    /// This is equivalent to calling [`ProcessesCollection::process_by_id`] for each possible
    /// ID.
    pub fn pids<'a>(
        &'a self,
    ) -> impl ExactSizeIterator<Item = ProcessesCollectionProc<'a, TExtr, TPud, TTud>> + 'a {
        let processes = self.processes.lock();

        processes
            .values()
            .cloned()
            .filter_map(|process| {
                if let Some(process) = process.upgrade() {
                    Some(ProcessesCollectionProc {
                        collection: self,
                        process: Some(process),
                        pid_tid_pool: &self.pid_tid_pool,
                    })
                } else {
                    None
                }
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
    ///
    /// This method is guaranteed to return `Some` for a process for which you already have an
    /// existing lock, or if you hold a lock to one of its threads. However, it can return `None`
    /// for a process that has crashed or finished before said crash or termination has been
    /// reported with the [`run`](ProcessesCollection::run) method.
    pub fn process_by_id(&self, pid: Pid) -> Option<ProcessesCollectionProc<TExtr, TPud, TTud>> {
        let processes = self.processes.lock();

        if let Some(p) = processes.get(&pid)?.upgrade() {
            debug_assert_eq!(p.pid, pid);
            Some(ProcessesCollectionProc {
                collection: self,
                process: Some(p),
                pid_tid_pool: &self.pid_tid_pool,
            })
        } else {
            None
        }
    }

    /// Returns a thread by its [`ThreadId`], if it exists and is not running.
    ///
    /// Only threads that are currently paused waiting to be resumed can be grabbed using this
    /// method.
    ///
    /// Additionally, the returned object holds an exclusive lock to this thread. Calling this
    /// method twice for the same thread will fail the second time.
    pub fn interrupted_thread_by_id(
        &self,
        id: ThreadId,
    ) -> Option<ProcessesCollectionThread<TExtr, TPud, TTud>> {
        let mut interrupted_threads = self.interrupted_threads.lock();

        if let Some((user_data, process)) = interrupted_threads.remove(&id) {
            Some(ProcessesCollectionThread {
                collection: self,
                process: Some(process),
                tid: id,
                pid_tid_pool: &self.pid_tid_pool,
                user_data: Some(user_data),
            })
        } else {
            None
        }
    }

    /// If the `process` passed as parameter is the last strong reference, then cleans the state
    /// of `self` for traces.
    fn try_report_process_death(&self, process: Arc<Process<TPud, TTud>>) {
        let mut process = match Arc::try_unwrap(process) {
            Ok(p) => p,
            Err(_) => return,
        };

        let _was_in = self.processes.lock().remove(&process.pid);
        debug_assert!(_was_in.is_some());
        debug_assert_eq!(_was_in.as_ref().unwrap().weak_count(), 0);
        debug_assert_eq!(_was_in.as_ref().unwrap().strong_count(), 0);

        debug_assert!(process.lock.get_mut().threads_to_resume.is_empty());

        let dead = process.lock.get_mut().dead.take().unwrap();
        self.death_reports.push((
            process.pid,
            process.user_data,
            dead.dead_threads,
            dead.outcome,
        ));
    }
}

impl<TExtr> Default for ProcessesCollectionBuilder<TExtr> {
    fn default() -> ProcessesCollectionBuilder<TExtr> {
        ProcessesCollectionBuilder {
            pid_tid_pool: IdPool::new(),
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
        self.pid_tid_pool.assign()
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
            pid_tid_pool: self.pid_tid_pool,
            execution_queue: SegQueue::new(),
            interrupted_threads: Spinlock::new(HashMap::with_capacity_and_hasher(
                PROCESSES_MIN_CAPACITY, // TODO: no
                Default::default(),
            )),
            processes: Spinlock::new(HashMap::with_capacity_and_hasher(
                PROCESSES_MIN_CAPACITY,
                Default::default(),
            )),
            extrinsics: self.extrinsics,
            extrinsics_id_assign: self.extrinsics_id_assign,
            death_reports: SegQueue::new(),
        }
    }
}

impl<'a, TExtr, TPud, TTud> ProcessesCollectionProc<'a, TExtr, TPud, TTud> {
    /// Returns the [`Pid`] of the process. Allows later retrieval by calling
    /// [`process_by_id`](ProcessesCollection::process_by_id).
    pub fn pid(&self) -> Pid {
        self.process.as_ref().unwrap().pid
    }

    /// Returns the user data that is associated to the process.
    pub fn user_data(&self) -> &TPud {
        &self.process.as_ref().unwrap().user_data
    }

    /// Adds a new thread to the process, starting the function with the given index and passing
    /// the given parameters.
    ///
    /// > **Note**: The "function ID" is the index of the function in the WASM module. WASM
    /// >           doesn't have function pointers. Instead, all the functions are part of a single
    /// >           global array of functions.
    // TODO: don't expose wasmi::RuntimeValue in the API
    pub fn start_thread(
        &self,
        fn_index: u32,
        params: Vec<wasmi::RuntimeValue>,
        user_data: TTud,
    ) -> Result<ThreadId, vm::StartErr> {
        let thread_id = self.pid_tid_pool.assign(); // TODO: check for duplicates
        let thread_data = Thread { thread_id };

        let mut process_state = self.process.as_ref().unwrap().lock.lock();

        process_state
            .vm
            .start_thread_by_id(fn_index, params, thread_data)?;

        process_state
            .threads_to_resume
            .push_back((thread_id, user_data, None));

        Ok(thread_id)
    }

    // TODO: bad API because of unique lock system for threads
    pub fn interrupted_threads(
        &self,
    ) -> impl Iterator<Item = ProcessesCollectionThread<'a, TExtr, TPud, TTud>> + 'a {
        let mut interrupted_threads = self.collection.interrupted_threads.lock();

        let list = interrupted_threads
            .drain_filter(|_, (_, p)| Arc::ptr_eq(p, self.process.as_ref().unwrap()))
            .collect::<Vec<_>>();

        let collection = self.collection;
        let pid_tid_pool = self.pid_tid_pool;
        list.into_iter().map(
            move |(tid, (user_data, process))| ProcessesCollectionThread {
                collection,
                process: Some(process),
                tid,
                pid_tid_pool,
                user_data: Some(user_data),
            },
        )
    }
}

impl<'a, TExtr, TPud, TTud> fmt::Debug for ProcessesCollectionProc<'a, TExtr, TPud, TTud> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: threads user datas
        f.debug_struct("ProcessesCollectionProc")
            .field("pid", &self.pid())
            //.field("user_data", self.user_data())     // TODO: requires &mut self :-/
            .finish()
    }
}

impl<'a, TExtr, TPud, TTud> Drop for ProcessesCollectionProc<'a, TExtr, TPud, TTud> {
    fn drop(&mut self) {
        self.collection
            .try_report_process_death(self.process.take().unwrap());
    }
}

impl<'a, TExtr, TPud, TTud> ProcessesCollectionThread<'a, TExtr, TPud, TTud> {
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
        self.process.as_ref().unwrap().pid
    }

    /// Returns the process this thread belongs to.
    pub fn process(&self) -> ProcessesCollectionProc<'a, TExtr, TPud, TTud> {
        ProcessesCollectionProc {
            collection: self.collection,
            process: self.process.clone(),
            pid_tid_pool: self.pid_tid_pool,
        }
    }

    pub fn read_memory(&self, offset: u32, size: u32) -> Result<Vec<u8>, ()> {
        // TODO: will block until any other thread to finish executing ; it isn't really possible
        // right now to do otherwise, as the WASM memory model isn't properly defined
        let lock = self.process.as_ref().unwrap().lock.lock();
        lock.vm.read_memory(offset, size)
    }

    /// Write the data at the given memory location.
    ///
    /// Returns an error if the range is invalid or out of range.
    pub fn write_memory(&self, offset: u32, value: &[u8]) -> Result<(), ()> {
        // TODO: will block until any other thread to finish executing ; it isn't really possible
        // right now to do otherwise, as the WASM memory model isn't properly defined
        let mut lock = self.process.as_ref().unwrap().lock.lock();
        lock.vm.write_memory(offset, value)
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
        let process = self.process.take().unwrap();
        let user_data = self.user_data.take().unwrap();

        let push_to_exec_q = {
            let mut process_state = process.lock.lock();
            let process_state = &mut *process_state;
            if let Some(death_state) = &mut process_state.dead {
                debug_assert!(process_state.vm.is_poisoned());
                death_state.dead_threads.push((self.tid, user_data));
                false
            } else {
                process_state
                    .threads_to_resume
                    .push_back((self.tid, user_data, value));
                true
            }
        };

        if push_to_exec_q {
            self.collection.execution_queue.push(process);
        } else {
            self.collection.try_report_process_death(process);
        }
    }
}

impl<'a, TExtr, TPud, TTud> Drop for ProcessesCollectionThread<'a, TExtr, TPud, TTud> {
    fn drop(&mut self) {
        let process = match self.process.take() {
            Some(p) => p,
            None => return,
        };

        // `self.user_data` is `None` if the thread has been resumed, and `Some` if it has been
        // dropped without being resumed.
        if let Some(user_data) = self.user_data.take() {
            let mut process_state = process.lock.lock();
            let process_state = &mut *process_state;

            if let Some(death_state) = &mut process_state.dead {
                debug_assert!(process_state.vm.is_poisoned());
                death_state.dead_threads.push((self.tid, user_data));
            } else {
                // TODO: fails debug_assert!(Arc::strong_count(&process) >= 2);
                let mut interrupted_threads = self.collection.interrupted_threads.lock();
                let _prev_in = interrupted_threads.insert(self.tid, (user_data, process.clone()));
                debug_assert!(_prev_in.is_none());
            }
        }

        // TODO: doesn't have to be called if we push the thread back in `interrupted_threads`,
        // but that gives borrowing errors
        self.collection.try_report_process_death(process);
    }
}

impl<'a, TExtr, TPud, TTud> fmt::Debug for ProcessesCollectionThread<'a, TExtr, TPud, TTud> {
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
    use super::{ProcessesCollectionBuilder, RunOneOutcome};
    use crate::sig;

    use hashbrown::HashSet;
    use std::{
        sync::{Arc, Barrier, Mutex},
        thread,
    };

    #[test]
    #[should_panic]
    fn panic_duplicate_extrinsic() {
        ProcessesCollectionBuilder::<()>::default()
            .with_extrinsic("foo", "test", sig!(()), ())
            .with_extrinsic("foo", "test", sig!(()), ());
    }

    #[test]
    fn basic() {
        let module = from_wat!(
            local,
            r#"(module
            (func $_start (result i32)
                i32.const 5)
            (export "_start" (func $_start)))
        "#
        );
        let processes = ProcessesCollectionBuilder::<()>::default().build();
        processes.execute(&module, (), ()).unwrap();
        match processes.run() {
            RunOneOutcome::ProcessFinished { outcome, .. } => {
                assert_eq!(outcome.unwrap(), Some(wasmi::RuntimeValue::I32(5)));
            }
            _ => panic!(),
        };
    }

    #[test]
    fn many_processes() {
        let module = from_wat!(
            local,
            r#"(module
            (import "" "test" (func $test (result i32)))
            (func $_start (result i32)
                call $test)
            (export "_start" (func $_start)))
        "#
        );
        let num_processes = 10000;
        let num_threads = 8;

        let processes = Arc::new(
            ProcessesCollectionBuilder::<i32>::default()
                .with_extrinsic("", "test", sig!(() -> I32), 98)
                .build(),
        );
        let mut spawned_pids = HashSet::<_, fnv::FnvBuildHasher>::default();
        for _ in 0..num_processes {
            let pid = processes.execute(&module, (), ()).unwrap().pid();
            assert!(spawned_pids.insert(pid));
        }

        let finished_pids = Arc::new(Mutex::new(Vec::new()));
        let start_barrier = Arc::new(Barrier::new(num_threads));
        let end_barrier = Arc::new(Barrier::new(num_threads + 1));

        for _ in 0..num_threads {
            let processes = processes.clone();
            let finished_pids = finished_pids.clone();
            let start_barrier = start_barrier.clone();
            let end_barrier = end_barrier.clone();
            thread::spawn(move || {
                start_barrier.wait();

                let mut local_finished = Vec::with_capacity(num_processes);
                loop {
                    match processes.run() {
                        RunOneOutcome::ProcessFinished { pid, outcome, .. } => {
                            assert_eq!(outcome.unwrap(), Some(wasmi::RuntimeValue::I32(1234)));
                            local_finished.push(pid);
                        }
                        RunOneOutcome::Interrupted {
                            thread, id: &98, ..
                        } => {
                            thread.resume(Some(wasmi::RuntimeValue::I32(1234)));
                        }
                        RunOneOutcome::Idle => break,
                        _ => panic!(),
                    };
                }

                finished_pids.lock().unwrap().extend(local_finished);
                end_barrier.wait();
            });
        }

        end_barrier.wait();
        for pid in finished_pids.lock().unwrap().drain(..) {
            assert!(spawned_pids.remove(&pid));
        }
        assert!(spawned_pids.is_empty());
    }

    // TODO: add fuzzing here
}
