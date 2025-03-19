// Copyright (C) 2019-2021  Pierre Krieger
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

//! Collection of VMs representing processes.
//!
//! This module contains most the of important logic related to parallelism in the scheduler.
//!
//! The [`ProcessesCollection`] struct contains a list of processes identified by [`Pid`]s and
//! threads identified by [`ThreadId`]s. You can add new processes by calling
//! [`ProcessesCollection::execute`].
//!
//! Call [`ProcessesCollection::run`] in order to find a thread in the collection that is ready
//! to be run, executing it, and obtain an event describing what has just happened. The function
//! is asynchronous, and if there is nothing to do then its corresponding `Future` will be
//! pending.
//!
//! # Interrupted threads
//!
//! If [`RunOneOutcome::Interrupted`] is returned, that means that the given thread has just
//! called (from within the VM) an external function and is now in an "interrupted" state waiting
//! for the call to that external function to be finished.
//!
//! You can:
//!
//! - Either process the call immediately and call [`ThreadAccess::resume`], passing
//! the return value of the call.
//! - Or decide to resume the thread later. You can drop the [`ThreadAccess`] object,
//! and later retrieve it by calling [`ProcessesCollection::interrupted_thread_by_id`].
//!
//! A [`ThreadAccess`] represents a "locked access" to a thread. Only one instance of
//! [`ThreadAccess`] for any given thread can exist simultaneously. Attempting to access the same
//! thread multiple times will result in an error, and the upper layers should be designed in such
//! a way that this is not necessary.
//!
//! # Locking processes
//!
//! One can access the state of a process through a [`ProcAccess`]. This struct can
//! be obtained through a [`ThreadAccess`], or by calling
//! [`ProcessesCollection::process_by_id`] or [`ProcessesCollection::processes`].
//!
//! Contrary to threads, multiple instances of [`ProcAccess`] can exist for the same
//! process.
//!
//! If a process finishes (either by normal termination or because of a crash), the emission of
//! the corresponding [`RunOneOutcome::ProcessFinished`] event will be delayed until no instance
//! of [`ProcAccess`] corresponding to that process exist anymore.

// Implementation notes.
//
// The ownership of each thread is passed between five different structures, as illustrated below.
//
//                                            +---> RunOneOutcome::ThreadFinished
//                                            |
//                                            +---> ProcessDeadState::dead_threads <-------+
//                                            |                               ^            |
//                                            |                               +---------+  |
//   +--------------------------------+ run   |    +--------------+                     |  |
//   |                                +-------+--->+              |                     |  |
//   | ProcessLock::threads_to_resume |            | ThreadAccess |                     |  |
//   |                                +<-----------+              |                     |  |
//   +--------------------------------+    resume  +----+-----+---+                     |  |
//                                                      ^     |                         |  |
//                                                      |     | ThreadAccess::Drop      |  |
//                                                      |     |                         |  |
//                                                      |     |                         |  |
//                             interrupted_thread_by_id |     +-------------------------+  |
//                                                      |     v                            |
//                                   +------------------+-----+-----------------+          |
//                                   | ProcessesCollection::interrupted_threads +----------+
//                                   +------------------------------------------+
//
// When a thread is created, it is inserted in `ProcessLock::threads_to_resume`. After it is
// executed, it either terminates or is given to the user as a `ThreadAccess`. If the user drops
// that `ThreadAccess`, the thread is moved to `ProcessesCollection::interrupted_threads` where it
// can later be extracted again with `interrupted_thread_by_id`.
//
// Whenever a thread is inserted in `ProcessLock::threads_to_resume`, the process itself is also
// inserted in `ProcessesCollection::execution_queue`. The number of times a process is in
// `execution_queue` must always be equal to the length of its `threads_to_resume` field.
//
// When a process needs to be terminated (because of `ProcessAccess::abort`, the main thread has
// returned, or an error has happened), we don't immediately report the termination to the user
// or remove the process from the state. Instead, the process is marked as "dying" by putting a
// `Some` in `ProcessLock::dead`.
//
// When a process is marked as dying, normal thread state transitions (between `threads_to_resume`,
// `ThreadAccess` and `interrupted_threads`) are hijacked. Whenever a state transition would
// normally occur, the thread is instead moved to `ProcessDeadState::dead_threads`. In other
// words, whenever a thread state transition needs to happen, we first check the process's dying
// flag.
//
// Processes are always manipulated through an `Arc`, and the lifetime of a process is tracked by
// that `Arc`. An `Arc<Process>` must never be silently dropped, but instead passed to
// `try_report_process_death` for destruction. If `try_report_process_death` is called with the
// last remaining `Arc` containing a process, the content of `dead_threads` is extracted and the
// information about the death of the process is pushed to `death_reports`.
//
// When a death report has been pushed in `death_reports`, the state is entirely clean from that
// process. The last step is to report that death to the user through a
// `RunOneOutcome::ProcessFinished`, which happens as soon as `run` is called.

use crate::{id_pool::IdPool, primitives::Signature, scheduler::vm, Pid, ThreadId};

use alloc::{
    borrow::Cow,
    boxed::Box,
    collections::VecDeque,
    string::String,
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::{
    fmt,
    future::Future,
    mem,
    pin::Pin,
    task::{Context, Poll},
};
use crossbeam_queue::SegQueue;
use fnv::FnvBuildHasher;
use hashbrown::{hash_map::Entry, HashMap};
use nohash_hasher::BuildNoHashHasher;
use spinning_top::Spinlock;

#[cfg(test)]
mod tests;
mod wakers;

/// Minimum capacity of the container of the list of processes.
///
/// If we shrink the container too much, then it will have to perform lots of allocations in order
/// to grow again in the future. We therefore avoid that situation.
const PROCESSES_MIN_CAPACITY: usize = 128;

/// Collection of multiple [`ProcessStateMachine`](vm::ProcessStateMachine)s grouped together in a
/// smart way.
///
/// See the module-level documentation for more information.
///
/// The generic parameters `TPud` and `TTud` are "user data"s that are stored respectively per
/// process and per thread, and allows the user to put extra information associated to a process
/// or a thread.
pub struct ProcessesCollection<TExtr, TPud, TTud> {
    /// Allocations of process IDs and thread IDs.
    pid_tid_pool: IdPool,

    /// Holds the list of task wakers to wake when an event is ready or that something is ready to
    /// run.
    wakers: wakers::Wakers,

    /// Queue of processes with at least one thread to run. Every time a thread starts or is
    /// resumed, its process gets pushed to the end of this queue. In other words, each process is
    /// in this queue `N` times, it means that `N` of its threads are ready to run. There isn't
    /// any unnecessary entry.
    // TODO: use something better than a naive round robin?
    execution_queue: SegQueue<Arc<Process<TPud, TTud>>>,

    /// List of threads waiting to be resumed, plus their user data and the process they belong to.
    /// Doesn't contains threads that have been locked by the user with
    /// [`ProcessesCollection::interrupted_thread_by_id`], as this method extracts the thread from
    /// the list.
    // TODO: find a solution for that mutex?
    // TODO: call shrink_to_fit from time to time?
    interrupted_threads:
        Spinlock<HashMap<ThreadId, (TTud, Arc<Process<TPud, TTud>>), BuildNoHashHasher<u64>>>,

    /// List of all processes currently alive.
    ///
    /// We hold `Weak`s to processes rather than `Arc`s. Processes are kept alive by the execution
    /// queue and the interrupted threads, thereby guaranteeing that they are alive only if they
    /// can potentially continue running.
    // TODO: find a solution for that mutex?
    processes: Spinlock<HashMap<Pid, Weak<Process<TPud, TTud>>, BuildNoHashHasher<u64>>>,

    /// List of functions that processes can call.
    /// The key of this map is an arbitrary `usize` that we pass to the WASM virtual machine.
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
        Result<Option<crate::WasmValue>, Trap>,
    )>,
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

/// Part of each process's state that is behind a mutex.
struct ProcessLock<TTud> {
    /// The actual Wasm virtual machine. Do not use if `dead` is `Some`.
    vm: vm::ProcessStateMachine<Thread>,

    /// Queue of threads that are ready to be resumed.
    threads_to_resume: VecDeque<(ThreadId, TTud, Option<crate::WasmValue>)>,

    /// If `Some`, then the process has been marked for death. The virtual machine must no longer
    /// be used (as it might be in a poisoned state), and we are in the process of collecting all
    /// the threads user datas into the [`ProcessDeadState`] before notifying the user.
    dead: Option<ProcessDeadState<TTud>>,
}

/// Additional optional state to a process if it's been marked for destruction.
struct ProcessDeadState<TTud> {
    /// List of dead thread that we will ultimately send to the user.
    dead_threads: Vec<(ThreadId, TTud)>,

    /// Why the process ended. Never modified once set.
    outcome: Result<Option<crate::WasmValue>, Trap>,
}

/// Additional data associated to a thread. Stored within the [`vm::ProcessStateMachine`].
struct Thread {
    /// Identifier of the thread.
    thread_id: ThreadId,
}

impl<TExtr, TPud, TTud> ProcessesCollection<TExtr, TPud, TTud> {
    /// Creates a new process from the given module.
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
        module: &[u8],
        proc_user_data: TPud,
        main_thread_user_data: TTud,
    ) -> Result<(ProcAccess<TExtr, TPud, TTud>, ThreadId), vm::NewErr> {
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
                        if expected_signature == obtained_signature {
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
        self.wakers.notify_one();

        let proc_lock = ProcAccess {
            collection: self,
            process: Some(process),
            pid_tid_pool: &self.pid_tid_pool,
        };

        Ok((proc_lock, main_thread_id))
    }

    /// Find a thread that is ready to be run.
    ///
    /// Which thread is picked is implementation-defined and no guarantee is made.
    pub fn run(&self) -> RunFuture<TExtr, TPud, TTud> {
        RunFuture(self, self.wakers.register())
    }

    /// Returns an iterator to all the processes that exist in the collection.
    ///
    /// This is equivalent to calling [`ProcessesCollection::process_by_id`] for each possible
    /// ID.
    pub fn processes<'a>(
        &'a self,
    ) -> impl ExactSizeIterator<Item = ProcAccess<'a, TExtr, TPud, TTud>> + 'a {
        let processes = self.processes.lock();

        // TODO: what if process is in death_reports?

        processes
            .values()
            .cloned()
            .filter_map(|process| {
                if let Some(process) = process.upgrade() {
                    Some(ProcAccess {
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
    pub fn process_by_id(&self, pid: Pid) -> Option<ProcAccess<TExtr, TPud, TTud>> {
        let processes = self.processes.lock();

        // TODO: what if process is in death_reports?

        if let Some(p) = processes.get(&pid)?.upgrade() {
            debug_assert_eq!(p.pid, pid);
            Some(ProcAccess {
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
    ) -> Option<ThreadAccess<TExtr, TPud, TTud>> {
        let mut interrupted_threads = self.interrupted_threads.lock();

        // TODO: what if thread has been moved in dead_threads?

        if let Some((user_data, process)) = interrupted_threads.remove(&id) {
            Some(ThreadAccess {
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
    /// of `self` and reports the process's death to the user.
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
        self.wakers.notify_one();
    }
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

impl<TExtr> ProcessesCollectionBuilder<TExtr> {
    /// Initializes a new builder using the given random seed.
    ///
    /// The seed is used in determine how [`Pid`]s are generated. The same seed will result in
    /// the same sequence of [`Pid`]s.
    pub fn with_seed(seed: [u8; 32]) -> ProcessesCollectionBuilder<TExtr> {
        ProcessesCollectionBuilder {
            pid_tid_pool: IdPool::with_seed(seed),
            extrinsics: Default::default(),
            extrinsics_id_assign: Default::default(),
        }
    }

    /// Allocates a `Pid` that will not be used by any process.
    ///
    /// > **Note**: As of the writing of this comment, this feature is only ever used to allocate
    /// >           `Pid`s that last forever. There is therefore no corresponding "unreserve_pid"
    /// >           method that frees such an allocated `Pid`. If there is ever a need to free
    /// >           these `Pid`s, such a method should be added.
    pub fn reserve_pid(&mut self) -> Pid {
        // Note that this function accepts `&mut self`. It could be `&self`, but that would
        // expose implementation details.
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
            wakers: wakers::Wakers::default(),
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

/// Outcome of the [`run`](ReadyToRun::run) function.
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
        outcome: Result<Option<crate::WasmValue>, Trap>,
    },

    /// A thread in a process has finished.
    ThreadFinished {
        /// Thread which has finished.
        thread_id: ThreadId,

        /// Process whose thread has finished.
        process: ProcAccess<'a, TExtr, TPud, TTud>,

        /// User data of the thread.
        user_data: TTud,

        /// Value returned by the function that was executed.
        value: Option<crate::WasmValue>,
    },

    /// The currently-executed function has been paused due to a call to an external function.
    ///
    /// This variant contains the identifier of the external function that is expected to be
    /// called, and its parameters. When you call [`resume`](ThreadAccess::resume)
    /// again, you must pass back the outcome of calling that function.
    Interrupted {
        /// Thread that has been interrupted.
        thread: ThreadAccess<'a, TExtr, TPud, TTud>,

        /// Identifier of the function to call. Corresponds to the value provided at
        /// initialization when resolving imports.
        id: &'a TExtr,

        /// Parameters of the function call.
        params: Vec<crate::WasmValue>,
    },

    /// Running the thread has resulted in a decision to terminate the process. A
    /// [`RunOneOutcome::ProcessFinished`] will soon be emitted.
    StartProcessAbort {
        /// Pid of the process that is going to finish soon.
        pid: Pid,
    },
}

/// Opaque error that happened during execution, such as an `unreachable` instruction.
#[derive(Debug, Clone)]
pub struct Trap {
    pub error: String,
}

/// Future that drives the [`ProcessesCollection::run`] method.
pub struct RunFuture<'a, TExtr, TPud, TTud>(
    &'a ProcessesCollection<TExtr, TPud, TTud>,
    wakers::Registration<'a>,
);

impl<'a, TExtr, TPud, TTud> Future for RunFuture<'a, TExtr, TPud, TTud> {
    type Output = RunFutureOut<'a, TExtr, TPud, TTud>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = &mut *self;

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
            if let Some((pid, user_data, dead_threads, outcome)) = this.0.death_reports.pop() {
                return Poll::Ready(RunFutureOut::Direct(RunOneOutcome::ProcessFinished {
                    pid,
                    user_data,
                    dead_threads,
                    outcome,
                }));
            }

            // We start by finding a process that is ready to run and lock it by extracting the
            // state machine.
            let process = match this.0.execution_queue.pop() {
                Some(p) => p,
                None => {
                    // Register the wake-up for when a new item is pushed to `execution_queue`.
                    this.1.set_waker(cx.waker());

                    // It is possible for an item to have been pushed to `execution_queue` right
                    // before we called `set_waker`. Try again to make sure the list is empty.
                    match this.0.execution_queue.pop() {
                        Some(p) => p,
                        None => return Poll::Pending,
                    }
                }
            };

            // "Lock" the process's state machine for examination.
            // TODO: this is ok right now because we only have a single thread per process
            let mut proc_state = process.lock.lock();

            // If the process was in `this.execution_queue`, then we are guaranteed that a
            // thread was ready.
            let (tid, thread_user_data, resume_value) =
                proc_state.threads_to_resume.pop_front().unwrap();
            // TODO: a lot of code in this module assumes one thread per process, which this debug_assert checks
            debug_assert!(proc_state.threads_to_resume.is_empty());

            // If the process is marked as dying, insert the thread in the dying state and see if
            // we can finalize the destruction.
            if let Some(proc_dead) = &mut proc_state.dead {
                proc_dead.dead_threads.push((tid, thread_user_data));
                drop(proc_state); // Drop the lock.
                this.0.try_report_process_death(process);
                continue;
            }

            drop(proc_state);

            break Poll::Ready(RunFutureOut::ReadyToRun(ReadyToRun {
                collection: this.0,
                process: Some(process),
                tid,
                thread_user_data: Some(thread_user_data),
                resume_value,
            }));
        }
    }
}

impl<'a, TExtr, TPud, TTud> Unpin for RunFuture<'a, TExtr, TPud, TTud> {}

/// Event returned by [`RunFuture`].
pub enum RunFutureOut<'a, TExtr, TPud, TTud> {
    /// Event directly generated.
    Direct(RunOneOutcome<'a, TExtr, TPud, TTud>),
    /// Ready to execute a bit of a thread.
    ReadyToRun(ReadyToRun<'a, TExtr, TPud, TTud>),
}

/// Ready to resume one of the threads of a process.
#[must_use]
pub struct ReadyToRun<'a, TExtr, TPud, TTud> {
    /// The parent object.
    collection: &'a ProcessesCollection<TExtr, TPud, TTud>,
    /// Process to execute.
    /// Always `Some` except during destruction.
    /// Since it isn't possible to safely hold an `Arc<Process>` and `SpinlockGuard<...>`
    /// referencing that `Arc<Process>` in the same struct, the `lock` field is force-locked while
    /// the `ReadyToRun` is alive.
    process: Option<Arc<Process<TPud, TTud>>>,
    /// Id of the thread that we are going to run.
    tid: ThreadId,
    /// User data of the thread. Temporarily extracted from the global state. Always `Some`,
    /// except right before destruction.
    thread_user_data: Option<TTud>,
    /// Value to feed to the virtual machine on resume.
    resume_value: Option<crate::WasmValue>,
}

impl<'a, TExtr, TPud, TTud> ReadyToRun<'a, TExtr, TPud, TTud> {
    /// Performs the actual execution.
    pub fn run(mut self) -> RunOneOutcome<'a, TExtr, TPud, TTud> {
        // Lock the process, this time to execute the virtual machine.
        let mut proc_state = self.process.as_ref().unwrap().lock.lock();

        // Now run a thread until something happens.
        // This takes most of the CPU time of this function.
        let run_outcome = {
            debug_assert!(!proc_state.vm.is_poisoned());
            let thread_index = (0..proc_state.vm.num_threads())
                .find(|n| proc_state.vm.thread(*n).unwrap().user_data().thread_id == self.tid)
                .unwrap();
            proc_state
                .vm
                .thread(thread_index)
                .unwrap()
                .run(self.resume_value)
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
                let mut dead_threads = vec![(
                    main_thread_user_data.thread_id,
                    self.thread_user_data.take().unwrap(),
                )];

                // TODO: possible deadlock?
                let mut threads = self.collection.interrupted_threads.lock();
                // TODO: O(n) complexity
                while let Some(tid) = threads
                    .iter()
                    .find(|(_, (_, p))| Arc::ptr_eq(self.process.as_ref().unwrap(), p))
                    .map(|(k, _)| *k)
                {
                    let (user_data, _) = threads.remove(&tid).unwrap();
                    dead_threads.push((tid, user_data));
                }

                proc_state.dead = Some(ProcessDeadState {
                    dead_threads,
                    outcome: Ok(return_value),
                });

                RunOneOutcome::StartProcessAbort {
                    pid: self.process.as_ref().unwrap().pid,
                }
            }

            // The thread has ended.
            Ok(vm::ExecOutcome::ThreadFinished {
                return_value,
                user_data,
                ..
            }) => {
                debug_assert!(Arc::strong_count(&self.process.as_ref().unwrap()) >= 2);
                drop(proc_state);
                RunOneOutcome::ThreadFinished {
                    thread_id: user_data.thread_id,
                    process: ProcAccess {
                        collection: self.collection,
                        process: Some(self.process.as_ref().unwrap().clone()),
                        pid_tid_pool: &self.collection.pid_tid_pool,
                    },
                    user_data: self.thread_user_data.take().unwrap(),
                    value: return_value,
                }
            }

            // Thread wants to call an extrinsic function.
            Ok(vm::ExecOutcome::Interrupted {
                mut thread,
                id,
                params,
            }) => {
                // TODO: check params against signature with a debug_assert?
                let extrinsic = match self.collection.extrinsics.get(id) {
                    Some(e) => e,
                    None => unreachable!(),
                };
                let tid = thread.user_data().thread_id;
                drop(proc_state);
                RunOneOutcome::Interrupted {
                    thread: ThreadAccess {
                        collection: self.collection,
                        process: Some(self.process.as_ref().unwrap().clone()),
                        tid,
                        pid_tid_pool: &self.collection.pid_tid_pool,
                        user_data: Some(self.thread_user_data.take().unwrap()),
                    },
                    id: extrinsic,
                    params,
                }
            }

            // An error happened during the execution. We kill the entire process.
            Ok(vm::ExecOutcome::Errored { error, mut thread }) => {
                // TODO: Vec::with_capacity?
                let mut dead_threads = vec![(
                    thread.user_data().thread_id,
                    self.thread_user_data.take().unwrap(),
                )];
                drop(thread);

                debug_assert!(proc_state.vm.is_poisoned());
                debug_assert!(proc_state.dead.is_none());

                // TODO: possible deadlock?
                let mut threads = self.collection.interrupted_threads.lock();
                // TODO: O(n) complexity
                while let Some(tid) = threads
                    .iter()
                    .find(|(_, (_, p))| Arc::ptr_eq(self.process.as_ref().unwrap(), p))
                    .map(|(k, _)| *k)
                {
                    let (user_data, _) = threads.remove(&tid).unwrap();
                    dead_threads.push((tid, user_data));
                }

                proc_state.dead = Some(ProcessDeadState {
                    dead_threads,
                    outcome: Err(Trap { error: error.error }),
                });

                RunOneOutcome::StartProcessAbort {
                    pid: self.process.as_ref().unwrap().pid,
                }
            }
        }
    }
}

impl<'a, TExtr, TPud, TTud> Drop for ReadyToRun<'a, TExtr, TPud, TTud> {
    fn drop(&mut self) {
        // In the situation where the user didn't call `run`, we push back the thread to the
        // queue.
        if let Some(thread_user_data) = self.thread_user_data.take() {
            let mut proc_state = self.process.as_ref().unwrap().lock.lock();

            proc_state.threads_to_resume.push_back((
                self.tid,
                thread_user_data,
                self.resume_value.take(),
            ));
            self.collection
                .execution_queue
                .push(self.process.as_ref().unwrap().clone());
            self.collection.wakers.notify_one();
        }

        self.collection
            .try_report_process_death(self.process.take().unwrap());
    }
}

/// Access to a process within the collection.
pub struct ProcAccess<'a, TExtr, TPud, TTud> {
    collection: &'a ProcessesCollection<TExtr, TPud, TTud>,
    process: Option<Arc<Process<TPud, TTud>>>,

    /// Reference to the same field in [`ProcessesCollection`].
    pid_tid_pool: &'a IdPool,
}

impl<'a, TExtr, TPud, TTud> ProcAccess<'a, TExtr, TPud, TTud> {
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
    pub fn start_thread(
        &self,
        fn_index: u32,
        params: Vec<crate::WasmValue>,
        user_data: TTud,
    ) -> Result<ThreadId, vm::ThreadStartErr> {
        let thread_id = self.pid_tid_pool.assign(); // TODO: check for duplicates
        let thread_data = Thread { thread_id };

        let mut process_state = self.process.as_ref().unwrap().lock.lock();

        process_state
            .vm
            .start_thread_by_id(fn_index, params, thread_data)?;

        process_state
            .threads_to_resume
            .push_back((thread_id, user_data, None));

        self.collection
            .execution_queue
            .push(self.process.as_ref().unwrap().clone());
        self.collection.wakers.notify_one();

        Ok(thread_id)
    }

    /// Marks the process as aborting.
    ///
    /// The termination will happen after all locks to this process have been released.
    ///
    /// Calling [`abort`](ProcAccess::abort) a second time or more has no effect.
    pub fn abort(&self) {
        let mut process_state = self.process.as_ref().unwrap().lock.lock();

        if process_state.dead.is_some() {
            return;
        }

        // TODO: possible deadlock?
        let mut threads = self.collection.interrupted_threads.lock();
        let mut dead_threads = Vec::new();
        // TODO: O(n) complexity
        while let Some(tid) = threads
            .iter()
            .find(|(_, (_, p))| Arc::ptr_eq(self.process.as_ref().unwrap(), p))
            .map(|(k, _)| *k)
        {
            let (user_data, _) = threads.remove(&tid).unwrap();
            dead_threads.push((tid, user_data));
        }

        process_state.dead = Some(ProcessDeadState {
            dead_threads,
            outcome: Err(Trap {
                // TODO: use an enum for Trap instead?
                error: String::from("Aborted"),
            }),
        });
    }
}

impl<'a, TExtr, TPud, TTud> fmt::Debug for ProcAccess<'a, TExtr, TPud, TTud>
where
    TPud: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ProcAccess")
            .field("pid", &self.pid())
            .field("user_data", self.user_data())
            .finish()
    }
}

impl<'a, TExtr, TPud, TTud> Clone for ProcAccess<'a, TExtr, TPud, TTud> {
    fn clone(&self) -> Self {
        ProcAccess {
            collection: self.collection,
            pid_tid_pool: self.pid_tid_pool,
            process: self.process.clone(),
        }
    }
}

impl<'a, TExtr, TPud, TTud> Drop for ProcAccess<'a, TExtr, TPud, TTud> {
    fn drop(&mut self) {
        self.collection
            .try_report_process_death(self.process.take().unwrap());
    }
}

/// Access to a thread within the collection.
pub struct ThreadAccess<'a, TExtr, TPud, TTud> {
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

/// Error while reading memory.
#[derive(Debug)]
pub struct OutOfBoundsError;

impl<'a, TExtr, TPud, TTud> ThreadAccess<'a, TExtr, TPud, TTud> {
    /// Returns the id of the thread. Allows later retrieval by calling
    /// [`thread_by_id`](ProcessesCollection::interrupted_thread_by_id).
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
    pub fn process(&self) -> ProcAccess<'a, TExtr, TPud, TTud> {
        ProcAccess {
            collection: self.collection,
            process: self.process.clone(),
            pid_tid_pool: self.pid_tid_pool,
        }
    }

    /// Write the data at the given memory location.
    ///
    /// Returns an error if the range is invalid or out of range.
    ///
    /// > **Important**: See also the remarks on [`ThreadAccess::write_memory`].
    ///
    pub fn read_memory(&self, offset: u32, size: u32) -> Result<Vec<u8>, OutOfBoundsError> {
        // TODO: if another thread of this process is running, this will block until it has
        // finished executing ; it isn't really possible right now to do otherwise, as the WASM
        // memory model isn't properly defined
        let lock = self.process.as_ref().unwrap().lock.lock();
        lock.vm
            .read_memory(offset, size)
            .map_err(|vm::OutOfBoundsError| OutOfBoundsError)
    }

    /// Write the data at the given memory location.
    ///
    /// Returns an error if the range is invalid or out of range.
    ///
    /// # About concurrency
    ///
    /// Memory writes made using this method are guaranteed to be visible later by calling
    /// [`read_memory`](ThreadAccess::read_memory) on the same thread.
    ///
    /// However, writes are not guaranteed to be visible by calling
    /// [`read_memory`](ThreadAccess::read_memory) on a different thread, even when
    /// they belong to the same process.
    ///
    /// It is only when the instance of [`ThreadAccess`] is
    /// [resume](ThreadAccess::resume)d that the writes are guaranteed to be made
    /// visible to the rest of the process. This means that it is legal, for example, for this
    /// method to keep a cache of the changes and flush it later.
    ///
    /// As such, just like for "actual threads", writing and reading the same memory from multiple
    /// different threads without any synchronization primitive (which resuming the thread
    /// provides) will lead to a race condition.
    ///
    pub fn write_memory(&mut self, offset: u32, value: &[u8]) -> Result<(), OutOfBoundsError> {
        // TODO: if another thread of this process is running, this will block until it has
        // finished executing ; it isn't really possible right now to do otherwise, as the WASM
        // memory model isn't properly defined
        let mut lock = self.process.as_ref().unwrap().lock.lock();
        lock.vm
            .write_memory(offset, value)
            .map_err(|vm::OutOfBoundsError| OutOfBoundsError)
    }

    /// Returns the user data that is associated to the thread.
    pub fn user_data(&self) -> &TTud {
        self.user_data.as_ref().unwrap()
    }

    /// Returns the user data that is associated to the thread.
    pub fn user_data_mut(&mut self) -> &mut TTud {
        self.user_data.as_mut().unwrap()
    }

    /// After [`RunOneOutcome::Interrupted`] is returned, use this function to feed back the value
    /// to use as the return type of the function that has been called.
    ///
    /// This releases the [`ThreadAccess`]. The thread can now potentially be run by
    /// calling [`ProcessesCollection::run`].
    pub fn resume(mut self, value: Option<crate::WasmValue>) {
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
            self.collection.wakers.notify_one();
        } else {
            self.collection.try_report_process_death(process);
        }
    }
}

impl<'a, TExtr, TPud, TTud> fmt::Debug for ThreadAccess<'a, TExtr, TPud, TTud> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ThreadAccess")
            .field("pid", &self.pid())
            .finish()
    }
}

impl<'a, TExtr, TPud, TTud> Drop for ThreadAccess<'a, TExtr, TPud, TTud> {
    fn drop(&mut self) {
        let process = match self.process.take() {
            Some(p) => p,
            None => return,
        };

        // `self.user_data` is `None` if the thread has been resumed, and `Some` if it has been
        // dropped without being resumed.
        if let Some(user_data) = self.user_data.take() {
            let mut process_state_lock = process.lock.lock();
            let process_state = &mut *process_state_lock;

            if let Some(death_state) = &mut process_state.dead {
                debug_assert!(process_state.vm.is_poisoned());
                death_state.dead_threads.push((self.tid, user_data));
            } else {
                let mut interrupted_threads = self.collection.interrupted_threads.lock();
                drop(process_state_lock);
                let _prev_in = interrupted_threads.insert(self.tid, (user_data, process));
                debug_assert!(_prev_in.is_none());
                return;
            }
        }

        self.collection.try_report_process_death(process);
    }
}
