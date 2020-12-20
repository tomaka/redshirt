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

//! Core system, alongside with support for native programs, and some predefined interfaces and
//! features.
//!
//! Natively handles the following interfaces:
//! TODO: indicate hashes
//! TODO: more details
//!
//! - `interface`.
//!

use crate::extrinsics;
use crate::module::{Module, ModuleHash};
use crate::native::{self, NativeProgramMessageIdWrite as _};
use crate::scheduler::{Core, CoreBuilder, CoreRunOutcome, NewErr};
use crate::InterfaceHash;

use alloc::{collections::VecDeque, format, vec::Vec};
use core::{
    convert::TryFrom as _, fmt, iter, mem, num::NonZeroU64, sync::atomic::Ordering, task::Poll,
};
use crossbeam_queue::SegQueue;
use futures::prelude::*;
use hashbrown::{hash_map::Entry, HashMap, HashSet};
use nohash_hasher::BuildNoHashHasher;
use redshirt_syscalls::{Decode, Encode, EncodedMessage, MessageId, Pid};
use spinning_top::Spinlock;

/// Main struct that handles a system, including the scheduler, program loader,
/// inter-process communication, and so on.
///
/// See [the module-level documentation](super) for more information.
pub struct System<'a, TExtr: extrinsics::Extrinsics> {
    /// Inner system with inter-process communications.
    core: Core<TExtr>,

    /// For each interface, which program is fulfilling it.
    interfaces: Spinlock<Interfaces>,

    /// Total number of processes that have been spawned since initialization.
    num_processes_started: atomic::Atomic<u64>,

    /// Total number of processes that have successfully ended since initialization.
    num_processes_finished: atomic::Atomic<u64>,

    /// Total number of processes that have ended because of a problem, since initialization.
    num_processes_trap: atomic::Atomic<u64>,

    /// Collection of programs. Each is assigned a `Pid` that is reserved within `core`.
    /// Can communicate with the WASM programs that are within `core`.
    native_programs: native::NativeProgramsCollection<'a>,

    /// Registration ID (i.e. index in [`Interfaces::registrations`]) that handles the `loader`
    /// interface, or `None` is no such program exists yet.
    // TODO: add timeout for loader interface availability?
    loader_registration_id: atomic::Atomic<Option<usize>>,

    /// List of programs to load if the loader interface handler is available.
    programs_to_load: SegQueue<ModuleHash>,

    /// "Virtual" pid for the process that sends messages towards the loader.
    load_source_virtual_pid: Pid,

    /// Set of messages that we emitted of requests to load a program from the loader interface.
    /// All these messages expect a `redshirt_loader_interface::ffi::LoadResponse` as answer.
    // TODO: call shink_to_fit from time to time
    loading_programs: Spinlock<HashSet<MessageId, BuildNoHashHasher<u64>>>,
}

#[derive(Debug)]
struct Interfaces {
    interfaces: HashMap<InterfaceHash, Interface, fnv::FnvBuildHasher>,
    registrations: slab::Slab<InterfaceRegistration>,
}

#[derive(Debug)]
enum Interface {
    /// Contains an index within [`Interfaces::registrations`].
    Registered(usize),
    NotRegistered {
        /// Messages emitted by programs and that haven't been accepted yet are pushed to this
        /// field.
        pending_accept: VecDeque<MessageId>,
    },
}

#[derive(Debug)]
struct InterfaceRegistration {
    pid: Pid,
    /// Messages of type `NextMessage` sent on the interface interface and that must be answered
    /// with the next interface message.
    queries: VecDeque<MessageId>,
    /// If [`InterfaceRegistration::queries`] is empty, messages emitted by programs and that
    /// haven't been accepted yet are pushed to this field.
    pending_accept: VecDeque<MessageId>,
}

/// Prototype for a [`System`].
pub struct SystemBuilder<'a, TExtr: extrinsics::Extrinsics> {
    /// Builder for the inner core.
    core: CoreBuilder<TExtr>,

    /// "Virtual" pid for the process that sends messages towards the loader.
    load_source_virtual_pid: Pid,

    /// Native programs.
    native_programs: native::NativeProgramsCollection<'a>,

    /// List of programs to start executing immediately after construction.
    startup_processes: Vec<Module>,

    /// Same field as [`System::programs_to_load`].
    programs_to_load: SegQueue<ModuleHash>,
}

/// Outcome of running the [`System`] once.
#[derive(Debug)]
pub enum SystemRunOutcome<'a, 'b, TExtr: extrinsics::Extrinsics> {
    /// A program has ended, either successfully or after an error.
    ProgramFinished {
        /// Identifier of the process that has stopped.
        pid: Pid,
        /// Either `Ok(())` if the main thread has ended, or the error that happened in the
        /// process.
        // TODO: change error type
        outcome: Result<(), wasmi::Error>,
    },
    /// A program has requested metrics from the kernel. Use the [`KernelDebugMetricsRequest`] to
    /// report them.
    KernelDebugMetricsRequest(KernelDebugMetricsRequest<'a, 'b, TExtr>),
}

#[derive(Debug)]
enum RunOnceOutcome<'a, 'b, TExtr: extrinsics::Extrinsics> {
    Report(SystemRunOutcome<'a, 'b, TExtr>),
    LoopAgain,
    LoopAgainNow,
}

impl<'a, TExtr> System<'a, TExtr>
where
    TExtr: extrinsics::Extrinsics,
{
    /// Start executing a program.
    pub fn execute(&self, program: &Module) -> Result<Pid, NewErr> {
        self.num_processes_started.fetch_add(1, Ordering::Relaxed);
        Ok(self.core.execute(program)?.0.pid())
    }

    /// Runs the [`System`] once and returns the outcome.
    ///
    /// > **Note**: For now, it can a long time for this `Future` to be `Ready` because it is also
    /// >           waiting for the native programs to produce events in case there's nothing to
    /// >           do. In other words, this function can be seen more as a generator that whose
    /// >           `Future` becomes `Ready` only when something needs to be notified.
    pub fn run<'b>(&'b self) -> impl Future<Output = SystemRunOutcome<'a, 'b, TExtr>> + 'b {
        // TODO: We use a `poll_fn` because async/await don't work in no_std yet.
        future::poll_fn(move |cx| {
            loop {
                // TODO: put an await here instead
                let run_once_outcome = {
                    let fut = self.run_once();
                    futures::pin_mut!(fut);
                    Future::poll(fut, cx)
                };

                if let Poll::Ready(RunOnceOutcome::Report(out)) = run_once_outcome {
                    return Poll::Ready(out);
                }

                if let Poll::Ready(RunOnceOutcome::LoopAgainNow) = run_once_outcome {
                    continue;
                }

                let next_event = self.native_programs.next_event();
                futures::pin_mut!(next_event);
                let event = match next_event.poll(cx) {
                    Poll::Ready(ev) => ev,
                    Poll::Pending => {
                        if let Poll::Ready(RunOnceOutcome::LoopAgain) = run_once_outcome {
                            continue;
                        }
                        return Poll::Pending;
                    }
                };

                match event {
                    native::NativeProgramsCollectionEvent::Emit {
                        interface,
                        emitter_pid,
                        message,
                        message_id_write,
                    } if interface == redshirt_interface_interface::ffi::INTERFACE => {
                        match redshirt_interface_interface::ffi::InterfaceMessage::decode(message) {
                            Ok(redshirt_interface_interface::ffi::InterfaceMessage::Register(
                                interface_hash,
                            )) => {
                                // Set the process as interface handler, if possible.
                                let result =
                                    self.set_interface_handler(&interface_hash, emitter_pid);

                                let response =
                                    redshirt_interface_interface::ffi::InterfaceRegisterResponse {
                                        result: result.clone(),
                                    };
                                if let Some(message_id_write) = message_id_write {
                                    let message_id = self.core.allocate_untracked_message();
                                    message_id_write.acknowledge(message_id);
                                    self.native_programs
                                        .message_response(message_id, Ok(response.encode()));
                                }
                            }
                            Ok(
                                redshirt_interface_interface::ffi::InterfaceMessage::NextMessage(
                                    registration_id,
                                ),
                            ) => {
                                let mut interfaces = self.interfaces.lock();

                                if let Some(message_id_write) = message_id_write {
                                    let message_id = self.core.allocate_untracked_message();
                                    message_id_write.acknowledge(message_id);

                                    if let Ok(registration_id) =
                                        usize::try_from(registration_id.get())
                                    {
                                        if let Some(registration) =
                                            interfaces.registrations.get_mut(registration_id)
                                        {
                                            if registration.pid == emitter_pid {
                                                registration.queries.push_back(message_id);
                                            } else {
                                                self.native_programs
                                                    .message_response(message_id, Err(()));
                                            }
                                        } else {
                                            self.native_programs
                                                .message_response(message_id, Err(()));
                                        }
                                    } else {
                                        self.native_programs.message_response(message_id, Err(()));
                                    }
                                }
                            }
                            Err(_) => {
                                if let Some(message_id_write) = message_id_write {
                                    let message_id = self.core.allocate_untracked_message();
                                    message_id_write.acknowledge(message_id);
                                    self.native_programs.message_response(message_id, Err(()));
                                }
                            }
                        }
                    }
                    native::NativeProgramsCollectionEvent::Emit {
                        interface,
                        emitter_pid,
                        message,
                        message_id_write,
                    } => {
                        // TODO:
                        todo!()
                        /*// The native programs want to emit a message in the kernel.
                        if let Some(message_id_write) = message_id_write {
                            let message_id =
                                self.core
                                    .emit_message_answer(emitter_pid, interface, message);
                            message_id_write.acknowledge(message_id);
                        } else {
                            self.core
                                .emit_message_no_answer(emitter_pid, interface, message);
                        }*/
                    }
                    native::NativeProgramsCollectionEvent::CancelMessage { message_id } => {
                        // TODO:
                        todo!()
                        // The native programs want to cancel a previously-emitted message.
                        //self.core.cancel_message(message_id);
                    }
                    native::NativeProgramsCollectionEvent::Answer { message_id, answer } => {
                        // TODO: could be a native program answer instead
                        self.core.answer_message(message_id, answer);
                    }
                }
            }
        })
    }

    async fn run_once<'b>(&'b self) -> RunOnceOutcome<'a, 'b, TExtr> {
        match self.core.run().await {
            CoreRunOutcome::ProgramFinished { pid, outcome, .. } => {
                // TODO: cancel interface registrations ; update loader_registration_id
                // TODO: notify interface registrations of process destruction

                if outcome.is_ok() {
                    self.num_processes_finished.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.num_processes_trap.fetch_add(1, Ordering::Relaxed);
                }

                return RunOnceOutcome::Report(SystemRunOutcome::ProgramFinished {
                    pid,
                    outcome: outcome.map(|_| ()).map_err(|err| err.into()),
                });
            }

            CoreRunOutcome::AnsweredMessage { message_id, answer } => {
                if self.loading_programs.lock().remove(&message_id) {
                    let redshirt_loader_interface::ffi::LoadResponse { result } =
                        Decode::decode(answer.unwrap()).unwrap();
                    // TODO: don't unwrap
                    let module = Module::from_bytes(&result.expect("loader returned error"))
                        .expect("module isn't proper wasm");
                    self.num_processes_started.fetch_add(1, Ordering::Relaxed);
                    match self.core.execute(&module) {
                        Ok(_) => {}
                        Err(_) => panic!(),
                    }
                } else {
                    self.native_programs.message_response(message_id, answer);
                }
            }

            CoreRunOutcome::InterfaceMessage {
                pid,
                needs_answer,
                immediate: _,
                message_id,
                interface,
            } if interface == redshirt_interface_interface::ffi::INTERFACE => {
                // Handling messages on the `interface` interface.
                let message = self.core.accept_interface_message(message_id);
                match redshirt_interface_interface::ffi::InterfaceMessage::decode(message) {
                    Ok(redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        interface_hash,
                    )) => {
                        // Set the process as interface handler, if possible.
                        let result = self.set_interface_handler(&interface_hash, pid);

                        let response =
                            redshirt_interface_interface::ffi::InterfaceRegisterResponse {
                                result: result.clone(),
                            };
                        if needs_answer {
                            self.core.answer_message(message_id, Ok(response.encode()));
                        }

                        // Special handling if the registered interface is the loader.
                        if interface_hash == redshirt_loader_interface::ffi::INTERFACE {
                            if let Ok(registration_id) = result {
                                return RunOnceOutcome::LoopAgainNow;
                            }
                        }
                    }
                    Ok(redshirt_interface_interface::ffi::InterfaceMessage::NextMessage(
                        registration_id,
                    )) => {
                        let mut interfaces = self.interfaces.lock();

                        if needs_answer {
                            if let Ok(registration_id) = usize::try_from(registration_id.get()) {
                                if let Some(registration) =
                                    interfaces.registrations.get_mut(registration_id)
                                {
                                    if registration.pid == pid {
                                        registration.queries.push_back(message_id);
                                    } else {
                                        self.core.answer_message(message_id, Err(()));
                                    }
                                } else {
                                    self.core.answer_message(message_id, Err(()));
                                }
                            } else {
                                self.core.answer_message(message_id, Err(()));
                            }
                        }
                    }
                    Err(_) => {
                        if needs_answer {
                            self.core.answer_message(message_id, Err(()));
                        }
                    }
                }
            }

            CoreRunOutcome::InterfaceMessage {
                pid,
                needs_answer,
                immediate,
                message_id,
                interface,
            } if interface == redshirt_kernel_debug_interface::ffi::INTERFACE => {
                // Handling messages on the `kernel_debug` interface.
                let message = self.core.accept_interface_message(message_id);
                if needs_answer {
                    if message.0.is_empty() {
                        return RunOnceOutcome::Report(
                            SystemRunOutcome::KernelDebugMetricsRequest(
                                KernelDebugMetricsRequest {
                                    system: self,
                                    message_id,
                                },
                            ),
                        );
                    } else {
                        self.core.answer_message(message_id, Err(()));
                    }
                }
            }

            CoreRunOutcome::InterfaceMessage {
                pid,
                needs_answer,
                immediate,
                message_id,
                interface,
            } => {
                let mut interfaces = self.interfaces.lock();
                let interfaces = &mut *interfaces; // Avoids borrow errors.

                match interfaces
                    .interfaces
                    .entry(interface.clone())
                    .or_insert_with(|| Interface::NotRegistered {
                        pending_accept: VecDeque::with_capacity(16), /* TODO: capacity */
                    }) {
                    Interface::Registered(registration_id) => {
                        let registration = &mut interfaces.registrations[*registration_id];
                        if let Some(interface_message) = registration.queries.pop_front() {
                            let message =
                                self.core.accept_interface_message_answerer(message_id, pid);
                            let answer =
                                redshirt_interface_interface::ffi::build_interface_notification(
                                    &interface,
                                    if needs_answer { Some(message_id) } else { None },
                                    pid,
                                    0,
                                    &message,
                                );
                            self.core.answer_message(
                                interface_message,
                                Ok(EncodedMessage(answer.into_bytes())),
                            );
                        } else if immediate {
                            self.core.reject_immediate_interface_message(message_id);
                        } else {
                            registration.pending_accept.push_back(message_id);
                        }
                    }
                    Interface::NotRegistered { pending_accept } => {
                        if immediate {
                            self.core.reject_immediate_interface_message(message_id);
                        } else {
                            // TODO: add some limit?
                            pending_accept.push_back(message_id);
                        }
                    }
                }
            }
        }

        RunOnceOutcome::LoopAgain
    }

    fn set_interface_handler(
        &self,
        interface_hash: &InterfaceHash,
        pid: Pid,
    ) -> Result<NonZeroU64, redshirt_interface_interface::ffi::InterfaceRegisterError> {
        let result = {
            let mut interfaces = self.interfaces.lock();
            let interfaces = &mut *interfaces;
            match interfaces.interfaces.entry(interface_hash.clone()) {
                Entry::Occupied(mut entry) => {
                    match entry.get_mut() {
                        Interface::Registered(_) =>
                            Err(redshirt_interface_interface::ffi::InterfaceRegisterError::AlreadyRegistered),
                        Interface::NotRegistered { pending_accept } => {
                            let id = interfaces.registrations.insert(InterfaceRegistration {
                                pid,
                                queries: VecDeque::with_capacity(16),  // TODO: be less magic with capacity
                                pending_accept: mem::replace(pending_accept, Default::default()),
                            });
                            entry.insert(Interface::Registered(id));
                            Ok(NonZeroU64::new(u64::try_from(id).unwrap()).unwrap())
                        }
                    }
                }
                Entry::Vacant(entry) => {
                    let id = interfaces.registrations.insert(InterfaceRegistration {
                        pid,
                        queries: VecDeque::with_capacity(16), // TODO: be less magic with capacity
                        pending_accept: VecDeque::with_capacity(16), // TODO: be less magic with capacity
                    });
                    entry.insert(Interface::Registered(id));
                    Ok(NonZeroU64::new(u64::try_from(id).unwrap()).unwrap())
                }
            }
        };

        if *interface_hash == redshirt_loader_interface::ffi::INTERFACE {
            if let Ok(registration_id) = result {
                while let Some(h) = self.programs_to_load.pop() {
                    todo!() // TODO:
                }
            }
        }

        // Special handling if the registered interface is the loader.
        if *interface_hash == redshirt_loader_interface::ffi::INTERFACE {
            if let Ok(registration_id) = result {
                self.loader_registration_id.store(
                    Some(usize::try_from(registration_id.get()).unwrap()),
                    Ordering::Release,
                );
            }
        }

        result
    }
}

/// Object to use to report kernel metrics to a requesting process.
#[must_use]
pub struct KernelDebugMetricsRequest<'a, 'b, TExtr: extrinsics::Extrinsics> {
    system: &'b System<'a, TExtr>,
    message_id: MessageId,
}

impl<'a, 'b, TExtr: extrinsics::Extrinsics> KernelDebugMetricsRequest<'a, 'b, TExtr> {
    /// Indicate the metrics. Must pass a Prometheus-compatible metrics.
    /// See [this document](https://prometheus.io/docs/instrumenting/exposition_formats/#text-format-details)
    /// for more information.
    ///
    /// The metrics will be concatenated with other metrics tracked internally by the `System`.
    pub fn respond(self, metrics: &str) {
        let mut metrics_bytes = metrics.as_bytes().to_vec();

        // `processes_started_total`
        metrics_bytes.extend_from_slice(
            b"# HELP processes_started_total Number of processes that have \
            been spawned since initialization.\n",
        );
        metrics_bytes.extend_from_slice(b"# TYPE processes_started_total counter\n");
        metrics_bytes.extend_from_slice(
            format!(
                "processes_started_total {}\n",
                self.system.num_processes_started.load(Ordering::Relaxed)
            )
            .as_bytes(),
        );
        metrics_bytes.extend_from_slice(b"\n");

        // `processes_ended_total`
        metrics_bytes.extend_from_slice(
            b"# HELP processes_ended_total Number of processes that have \
            ended, since initialization.\n",
        );
        metrics_bytes.extend_from_slice(b"# TYPE processes_ended_total counter\n");
        metrics_bytes.extend_from_slice(
            format!(
                "processes_ended_total{{reason=\"graceful\"}} {}\n",
                self.system.num_processes_finished.load(Ordering::Relaxed)
            )
            .as_bytes(),
        );
        metrics_bytes.extend_from_slice(
            format!(
                "processes_ended_total{{reason=\"crash\"}} {}\n",
                self.system.num_processes_trap.load(Ordering::Relaxed)
            )
            .as_bytes(),
        );
        metrics_bytes.extend_from_slice(b"\n");

        // TODO: add more metrics?

        let response = EncodedMessage(metrics_bytes);
        self.system
            .core
            .answer_message(self.message_id, Ok(response));
    }
}

impl<'a, 'b, TExtr: extrinsics::Extrinsics> fmt::Debug
    for KernelDebugMetricsRequest<'a, 'b, TExtr>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("KernelDebugMetricsRequest").finish()
    }
}

impl<'a, TExtr> SystemBuilder<'a, TExtr>
where
    TExtr: extrinsics::Extrinsics,
{
    /// Starts a new builder.
    pub fn new(extrinsics: TExtr) -> Self {
        let mut core = CoreBuilder::new();
        let load_source_virtual_pid = core.reserve_pid();

        SystemBuilder {
            core,
            startup_processes: Vec::new(),
            load_source_virtual_pid,
            programs_to_load: SegQueue::new(),
            native_programs: native::NativeProgramsCollection::new(),
        }
    }

    /// Registers native code that can communicate with the WASM programs.
    pub fn with_native_program<T>(mut self, program: T) -> Self
    where
        T: Send + Sync + 'a,
        for<'r> &'r T: native::NativeProgramRef<'r>,
    {
        self.native_programs.push(self.core.reserve_pid(), program);
        self
    }

    /// Adds a process to the list of processes that the [`System`] must start as part of the
    /// startup process.
    ///
    /// > **Note**: The startup processes are started in the order in which they are added here,
    /// >           but you should not rely on this fact for making the system work.
    ///
    /// By default, the list is empty. Should at least contain a process that handles the `loader`
    /// interface.
    pub fn with_startup_process(mut self, process: impl Into<Module>) -> Self {
        let process = process.into();
        self.startup_processes.push(process);
        self
    }

    /// Shortcut for calling [`with_main_program`](SystemBuilder::with_main_program) multiple
    /// times.
    pub fn with_main_programs(self, hashes: impl IntoIterator<Item = ModuleHash>) -> Self {
        for hash in hashes {
            self.programs_to_load.push(hash);
        }
        self
    }

    /// Adds a program that the [`System`] must execute after startup. Can be called multiple times
    /// to add multiple programs.
    ///
    /// The program will be loaded through the `loader` interface. The loading starts as soon as
    /// the `loader` interface has been registered by one of the processes passed to
    /// [`with_startup_process`](SystemBuilder::with_startup_process).
    ///
    /// Messages are sent to the `loader` interface in the order in which this function has been
    /// called. The implementation of `loader`, however, might not deliver the responses in the
    /// same order.
    pub fn with_main_program(self, hash: ModuleHash) -> Self {
        self.with_main_programs(iter::once(hash))
    }

    /// Builds the [`System`].
    ///
    /// Returns an error if any of the programs passed through
    /// [`SystemBuilder::with_startup_process`] fails to start.
    pub fn build(self) -> Result<System<'a, TExtr>, NewErr> {
        let core = self.core.build();

        let num_processes_started = u64::try_from(self.startup_processes.len()).unwrap();
        for program in self.startup_processes {
            core.execute(&program)?;
        }

        Ok(System {
            core,
            load_source_virtual_pid: self.load_source_virtual_pid,
            interfaces: Spinlock::new(Interfaces {
                interfaces: Default::default(),
                registrations: {
                    // Registration IDs are of the type `NonZeroU64`.
                    // The list of registrations starts with an entry at index `0` in order for
                    // generated registration IDs to never be equal to 0.
                    let mut registrations = slab::Slab::default();
                    let _id = registrations.insert(InterfaceRegistration {
                        pid: Pid::try_from(1234).unwrap(), // TODO: ?!
                        queries: VecDeque::new(),
                        pending_accept: VecDeque::new(),
                    });
                    assert_eq!(_id, 0);
                    registrations
                },
            }),
            num_processes_started: atomic::Atomic::new(num_processes_started),
            num_processes_finished: atomic::Atomic::new(0),
            num_processes_trap: atomic::Atomic::new(0),
            native_programs: self.native_programs,
            loader_registration_id: atomic::Atomic::new(None),
            loading_programs: Spinlock::new(Default::default()),
            programs_to_load: self.programs_to_load,
        })
    }
}

impl<'a, TExtr> Default for SystemBuilder<'a, TExtr>
where
    TExtr: extrinsics::Extrinsics + Default,
{
    fn default() -> Self {
        SystemBuilder::new(Default::default())
    }
}

#[cfg(test)]
mod tests {
    use crate::extrinsics;

    #[test]
    fn send_sync() {
        fn is_send_sync<T: Send + Sync>() {}
        is_send_sync::<super::System<extrinsics::NoExtrinsics>>()
    }
}
