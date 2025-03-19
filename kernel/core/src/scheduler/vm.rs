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

use crate::{primitives::Signature, ValueType, WasmValue};

use alloc::{
    borrow::{Cow, ToOwned as _},
    boxed::Box,
    format,
    string::{String, ToString as _},
    vec::Vec,
};
use core::{
    cell::RefCell,
    convert::{TryFrom as _, TryInto},
    fmt,
};
use itertools::Itertools as _;
use smallvec::SmallVec;

/// WASMI state machine dedicated to a process.
///
/// # Initialization
///
/// Initializing a state machine is done by passing a module, in other words a WASM binary.
///
/// The module might contain a list of functions to import and that the initialization process
/// must resolve. When such an import is encountered, the closure passed to the
/// [`new`](ProcessStateMachine::new) function is invoked and must return an opaque integer decided
/// by the user. This integer is later passed back to the user of this struct in the situation when
/// the state machine invokes that external function.
///
/// # Threads
///
/// This struct is composed of one or multiple threads. When initialized, the VM starts with a
/// single thread at the start of the "main" function of the WASM module.
///
/// In order to run the VM, grab a thread by calling [`ProcessStateMachine::threads`], then call
/// [`Thread::run`]. The thread will then run until it either finishes (in which case the thread
/// is then destroyed), or attempts to call an imported function.
///
/// TODO: It is intended that in the future the `run` function stops after a certain period of
/// time has elapsed, in order to do preemptive multithreading. This requires a lot of changes in
/// the interpreter, and isn't going to happen any time soon.
///
/// The [`run`](Thread::run) method requires passing a value. The first time you call
/// [`run`](Thread::run) for any given thread, you must pass the value `None`. If that thread is
/// then interrupted by a call to an imported function, you must execute the imported function and
/// pass its return value the next time you call [`run`](Thread::run).
///
/// The generic parameter of this struct is some userdata that is associated with each thread.
/// You must pass a value when creating a thread, and can retreive it later by calling
/// [`user_data`](Thread::user_data) or [`into_user_data`](Thread::into_user_data).
///
/// # Poisoning
///
/// If the main thread stops, or if any thread encounters an error, then the VM moves into a
/// "poisoned" state. It is then no longer possible to run anything in it. Threads are kept alive
/// so that you can examine their state, but attempting to call [`run`](Thread::run) will return
/// an error.
///
/// # Single-threaded-ness
///
/// The [`ProcessStateMachine`] is single-threaded. In other words, the VM can only ever run one
/// thread simultaneously. This might change in the future.
///
pub struct ProcessStateMachine<T> {
    /// Wasmi store.
    store: wasmi::Store<()>,

    /// An instance of the module.
    instance: wasmi::Instance,

    /// Memory of the module instantiation.
    ///
    /// Contains `None` if there's no memory.
    ///
    /// Right now we only support one unique `Memory` object per process.
    memory: Option<wasmi::Memory>,

    /// List of threads that this process is running.
    // TODO: use a Slab instead
    threads: SmallVec<[ThreadState<T>; 4]>,

    /// If true, the state machine is in a poisoned state and cannot run any code anymore.
    is_poisoned: bool,
}

/// State of a single thread within the VM.
struct ThreadState<T> {
    /// State of the thread execution.
    /// Always `Some`, but wrapped within an `Option` to be temporarily extracted.
    execution: Option<ThreadExecution>,

    /// Where the return value of the execution will be stored.
    /// While this could be regenerated every time `run` is called, it is instead kept in the
    /// `Interpreter` struct for convenience.
    dummy_output_value: Option<wasmi::Val>,

    /// Opaque user data associated with the thread.
    user_data: T,
}

enum ThreadExecution {
    /// Thread has been created but hasn't been started yet.
    NotStarted(wasmi::Func, Vec<wasmi::Val>),

    /// Thread has been started.
    Started(wasmi::ResumableInvocation),
}

/// Access to a thread within the virtual machine.
pub struct Thread<'a, T> {
    /// Reference to the parent object.
    vm: &'a mut ProcessStateMachine<T>,

    // Index within [`ProcessStateMachine::threads`] of the thread we are referencing.
    index: usize,
}

/// Outcome of the [`run`](Thread::run) function.
#[derive(Debug)]
pub enum ExecOutcome<'a, T> {
    /// A thread has finished. The thread no longer exists in the list.
    ///
    /// If this was the main thread (i.e. `thread_index` is 0), then the state machine is now in
    /// a poisoned state, and calling [`is_poisoned`](ProcessStateMachine::is_poisoned) will
    /// return true.
    ///
    /// > **Note**: Keep in mind that how you want to react to this event is probably very
    /// >           different depending on whether `thread_index` is 0. If this is the main thread,
    /// >           then the entire process is now stopped.
    ///
    ThreadFinished {
        /// Index of the thread that has finished.
        thread_index: usize,

        /// Return value of the thread function.
        return_value: Option<WasmValue>,

        /// User data that was stored within the thread.
        user_data: T,
    },

    /// The currently-executed thread has been paused due to a call to an external function.
    ///
    /// This variant contains the identifier of the external function that is expected to be
    /// called, and its parameters. When you call [`run`](Thread::run) again, you must pass back
    /// the outcome of calling that function.
    ///
    /// > **Note**: The type of the return value of the function is called is not specified, as the
    /// >           user is supposed to know it based on the identifier. It is an error to call
    /// >           [`run`](Thread::run) with a value of the wrong type.
    Interrupted {
        /// Thread that was interrupted.
        thread: Thread<'a, T>,

        /// Identifier of the function to call. Corresponds to the value provided at
        /// initialization when resolving imports.
        id: usize,

        /// Parameters of the function call.
        params: Vec<WasmValue>,
    },

    /// The currently-executed function has finished with an error. The state machine is now in a
    /// poisoned state.
    ///
    /// Calling [`is_poisoned`](ProcessStateMachine::is_poisoned) will return true.
    Errored {
        /// Thread that error'd.
        thread: Thread<'a, T>,

        /// Error that happened.
        error: Trap,
    },
}

/// Opaque error that happened during execution, such as an `unreachable` instruction.
#[derive(Debug, Clone)]
pub struct Trap {
    pub error: String,
}

/// Error that can happen when initializing a VM.
#[derive(Debug)]
pub enum NewErr {
    /// Wasm bytecode is invalid.
    InvalidWasm(String),
    /// Failed to resolve a function imported by the module.
    UnresolvedFunctionImport {
        /// Name of the function that was unresolved.
        function: String,
        /// Name of module associated with the unresolved function.
        module_name: String,
    },
    /// The "_start" symbol doesn't exist, or isn't a function, or doesn't have the expected
    /// signature.
    BadStartFunction,
    /// A memory object has both been imported and exported.
    MultipleMemoriesNotSupported,
    /// Failed to allocate memory for the virtual machine.
    CouldntAllocateMemory,
    /// The Wasm module requires importing a global or a table, which isn't supported.
    ImportTypeNotSupported,
    /// Error while instantiating the WebAssembly module.
    Instantiation {
        /// Opaque error message.
        error: String,
    },
}

/// Error that can happen when starting a new thread.
#[derive(Debug)]
pub enum ThreadStartErr {
    /// The state machine is poisoned and cannot run anymore.
    Poisoned,
    /// Couldn't find the requested function.
    FunctionNotFound,
    /// The requested function has been found in the list of exports, but it is not a function.
    NotAFunction,
    /// The signature of the function uses types that aren't supported.
    SignatureNotSupported,
    /// The types of the provided parameters don't match the signature.
    InvalidParameters,
}

/// Error while reading memory.
#[derive(Debug)]
pub struct OutOfBoundsError;

/// Error that can happen when resuming the execution of a function.
#[derive(Debug)]
pub enum RunErr {
    /// The state machine is poisoned.
    Poisoned,
    /// Passed a wrong value back.
    BadValueTy {
        /// Type of the value that was expected.
        expected: Option<ValueType>,
        /// Type of the value that was actually passed.
        obtained: Option<ValueType>,
    },
}

impl<T> ProcessStateMachine<T> {
    /// Creates a new process state machine from the given module.
    ///
    /// The closure is called for each import that the module has. It must assign a number to each
    /// import, or return an error if the import can't be resolved. When the VM calls one of these
    /// functions, this number will be returned back in order for the user to know how to handle
    /// the call.
    ///
    /// A single main thread (whose user data is passed by parameter) is automatically created and
    /// is paused at the start of the "_start" function of the module.
    pub fn new(
        module: &[u8],
        main_thread_user_data: T,
        mut symbols: impl FnMut(&str, &str, &Signature) -> Result<usize, ()>,
    ) -> Result<Self, NewErr> {
        let engine = {
            let config = wasmi::Config::default();
            // TODO: add config here?
            wasmi::Engine::new(&config)
        };

        let module = wasmi::Module::new(&engine, module.as_ref())
            .map_err(|err| NewErr::InvalidWasm(err.to_string()))?;

        let mut store = wasmi::Store::new(&engine, ());

        let mut linker = wasmi::Linker::<()>::new(&engine);
        let mut imported_memories = Vec::with_capacity(1);

        for import in module.imports() {
            match import.ty() {
                wasmi::ExternType::Func(func_type) => {
                    // Note that if `Signature::try_from` fails, a `UnresolvedFunctionImport` is
                    // also returned. This is because it is not possible for the function to
                    // resolve anyway if its signature can't be represented.
                    let function_index =
                        match Signature::try_from(func_type)
                            .ok()
                            .and_then(|conv_signature| {
                                symbols(import.module(), import.name(), &conv_signature).ok()
                            }) {
                            Some(i) => i,
                            None => {
                                return Err(NewErr::UnresolvedFunctionImport {
                                    module_name: import.module().to_owned(),
                                    function: import.name().to_owned(),
                                });
                            }
                        };

                    // `func_new` returns an error in case of duplicate definition. Since we
                    // enumerate over the imports, this can't happen.
                    linker
                        .func_new(
                            import.module(),
                            import.name(),
                            func_type.clone(),
                            move |_caller, parameters, _ret| {
                                Err(wasmi::Error::host(InterruptedTrap {
                                    function_index,
                                    parameters: parameters
                                        .iter()
                                        .map(|v| WasmValue::try_from(v).unwrap())
                                        .collect(),
                                }))
                            },
                        )
                        .unwrap();
                }
                wasmi::ExternType::Memory(memory_type) => {
                    let memory = wasmi::Memory::new(&mut store, *memory_type)
                        .map_err(|_| NewErr::CouldntAllocateMemory)?;
                    imported_memories.push(memory);

                    // `define` returns an error in case of duplicate definition. Since we
                    // enumerate over the imports, this can't happen.
                    linker
                        .define(import.module(), import.name(), memory)
                        .unwrap();
                }
                wasmi::ExternType::Global(_) | wasmi::ExternType::Table(_) => {
                    return Err(NewErr::ImportTypeNotSupported);
                }
            }
        }

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|err| NewErr::Instantiation {
                error: err.to_string(),
            })?
            // TODO: implement the special start function
            .start(&mut store)
            .map_err(|_| todo!())?;

        let memory = imported_memories
            .into_iter()
            .chain(instance.exports(&store).filter_map(|exp| exp.into_memory()))
            .at_most_one()
            .map_err(|_| NewErr::MultipleMemoriesNotSupported)?;

        let mut state_machine = ProcessStateMachine {
            store,
            instance,
            memory,
            is_poisoned: false,
            threads: SmallVec::new(),
        };

        // Try to start executing `_start`.
        match state_machine.start_thread_by_name("_start", &[][..], main_thread_user_data) {
            Ok(_) => {}
            Err((
                ThreadStartErr::FunctionNotFound
                | ThreadStartErr::NotAFunction
                | ThreadStartErr::InvalidParameters
                | ThreadStartErr::SignatureNotSupported,
                _,
            )) => return Err(NewErr::BadStartFunction),
            Err((ThreadStartErr::Poisoned, _)) => unreachable!(),
        };

        Ok(state_machine)
    }

    /// Returns true if the state machine is in a poisoned state and cannot run anymore.
    pub fn is_poisoned(&self) -> bool {
        self.is_poisoned
    }

    /// Starts executing a function. Immediately pauses the execution and puts it in an
    /// interrupted state.
    ///
    /// You should call [`run`](Thread::run) afterwards with a value of `None`.
    ///
    /// > **Note**: The "function ID" is the index of the function in the WASM module. WASM
    /// >           doesn't have function pointers. Instead, all the functions are part of a single
    /// >           global array of functions.
    pub fn start_thread_by_id(
        &mut self,
        function_id: u32,
        params: impl IntoIterator<Item = WasmValue>,
        user_data: T,
    ) -> Result<Thread<T>, ThreadStartErr> {
        if self.is_poisoned {
            return Err(ThreadStartErr::Poisoned);
        }

        // TODO: re-implement
        return Err(ThreadStartErr::FunctionNotFound);
    }

    /// Same as [`start_thread_by_id`](ProcessStateMachine::start_thread_by_id), but executes a
    /// symbol by name.
    fn start_thread_by_name(
        &mut self,
        symbol_name: &str,
        params: &[WasmValue],
        user_data: T,
    ) -> Result<Thread<T>, (ThreadStartErr, T)> {
        if self.is_poisoned {
            return Err((ThreadStartErr::Poisoned, user_data));
        }

        let func_to_call = match self.instance.get_export(&self.store, symbol_name) {
            Some(wasmi::Extern::Func(function)) => {
                // Try to convert the signature of the function to call, in order to make sure
                // that the type of parameters and return value are supported.
                let Ok(signature) = Signature::try_from(function.ty(&self.store)) else {
                    return Err((ThreadStartErr::SignatureNotSupported, user_data));
                };

                // Check whether the types of the parameters are correct.
                // This is necessary to do manually because for API purposes the call immediately
                //starts, while in the internal implementation it doesn't actually.
                if params.len() != signature.parameters().len() {
                    return Err((ThreadStartErr::InvalidParameters, user_data));
                }
                for (obtained, expected) in params.iter().zip(signature.parameters()) {
                    if obtained.ty() != *expected {
                        return Err((ThreadStartErr::InvalidParameters, user_data));
                    }
                }

                function
            }
            Some(_) => return Err((ThreadStartErr::NotAFunction, user_data)),
            None => return Err((ThreadStartErr::FunctionNotFound, user_data)),
        };

        let dummy_output_value = {
            let func_to_call_ty = func_to_call.ty(&self.store);
            let list = func_to_call_ty.results();
            // We don't support more than one return value. This is enforced by verifying the
            // function signature above.
            debug_assert!(list.len() <= 1);
            list.first().map(|item| match *item {
                wasmi::core::ValType::I32 => wasmi::Val::I32(0),
                wasmi::core::ValType::I64 => wasmi::Val::I64(0),
                wasmi::core::ValType::F32 => wasmi::Val::F32(0.0f32.into()),
                wasmi::core::ValType::F64 => wasmi::Val::F64(0.0.into()),
                _ => unreachable!(),
            })
        };

        self.threads.push(ThreadState {
            execution: Some(ThreadExecution::NotStarted(
                func_to_call,
                params
                    .iter()
                    .map(|v| wasmi::Val::from(*v))
                    .collect::<Vec<_>>(),
            )),
            dummy_output_value,
            user_data,
        });

        let thread_id = self.threads.len() - 1;
        Ok(Thread {
            vm: self,
            index: thread_id,
        })
    }

    /// Returns the number of threads that are running.
    pub fn num_threads(&self) -> usize {
        self.threads.len()
    }

    /// Returns the thread with the given index. The index is between `0` and
    /// [`num_threads`](ProcessStateMachine::num_threads).
    ///
    /// The main thread is always index `0`, unless it has terminated.
    ///
    /// Keep in mind that when a thread finishes, all the indices above its index shift by one.
    ///
    /// Returns `None` if the index is superior or equal to what
    /// [`num_threads`](ProcessStateMachine::num_threads) would return.
    pub fn thread(&mut self, index: usize) -> Option<Thread<T>> {
        if index < self.threads.len() {
            Some(Thread { vm: self, index })
        } else {
            None
        }
    }

    /// Consumes this VM and returns all the remaining threads' user datas.
    pub fn into_user_datas(self) -> impl ExactSizeIterator<Item = T> {
        self.threads.into_iter().map(|thread| thread.user_data)
    }

    /// Copies the given memory range into a `Vec<u8>`.
    ///
    /// Returns an error if the range is invalid or out of range.
    // TODO: make more zero-cost?
    pub fn read_memory(&self, offset: u32, size: u32) -> Result<Vec<u8>, OutOfBoundsError> {
        let mem = match self.memory.as_ref() {
            Some(m) => m,
            None => return Err(OutOfBoundsError),
        };

        let offset = usize::try_from(offset).map_err(|_| OutOfBoundsError)?;
        let size = usize::try_from(size).map_err(|_| OutOfBoundsError)?;

        let data = mem.data(&self.store);
        if offset + size > data.len() {
            return Err(OutOfBoundsError);
        }

        Ok(data[offset..][..size].to_vec())
    }

    /// Write the data at the given memory location.
    ///
    /// Returns an error if the range is invalid or out of range.
    pub fn write_memory(&mut self, offset: u32, value: &[u8]) -> Result<(), OutOfBoundsError> {
        let mem = match self.memory.as_ref() {
            Some(m) => m,
            None => return Err(OutOfBoundsError),
        };

        mem.write(
            &mut self.store,
            usize::try_from(offset).map_err(|_| OutOfBoundsError)?,
            value,
        )
        .map_err(|_| OutOfBoundsError)
    }
}

// The fields related to `wasmi` do not implement `Send` because they use `std::rc::Rc`. `Rc`
// does not implement `Send` because incrementing/decrementing the reference counter from
// multiple threads simultaneously would be racy. It is however perfectly sound to move all the
// instances of `Rc`s at once between threads, which is what we're doing here.
//
// This importantly means that we should never return a `Rc` (even by reference) across the API
// boundary.
// TODO: really annoying to have to use unsafe code
unsafe impl<T: Send> Send for ProcessStateMachine<T> {}

impl<T> fmt::Debug for ProcessStateMachine<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list().entries(self.threads.iter()).finish()
    }
}

impl<T> fmt::Debug for ThreadState<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Thread").field(&self.user_data).finish()
    }
}

impl<'a, T> Thread<'a, T> {
    /// Starts or continues execution of this thread.
    ///
    /// If this is the first call you call [`run`](Thread::run) for this thread, then you must pass
    /// a value of `None`.
    /// If, however, you call this function after a previous call to [`run`](Thread::run) that was
    /// interrupted by an external function call, then you must pass back the outcome of that call.
    pub fn run(mut self, value: Option<WasmValue>) -> Result<ExecOutcome<'a, T>, RunErr> {
        if self.vm.is_poisoned {
            return Err(RunErr::Poisoned);
        }

        let thread_state = &mut self.vm.threads[self.index];

        let outputs_storage_ptr =
            if let Some(output_storage) = thread_state.dummy_output_value.as_mut() {
                &mut core::array::from_mut(output_storage)[..]
            } else {
                &mut []
            };

        let result = match thread_state.execution.take() {
            Some(ThreadExecution::NotStarted(func, params)) => {
                if let Some(value) = value.as_ref() {
                    return Err(RunErr::BadValueTy {
                        expected: None,
                        obtained: Some(value.ty()),
                    });
                }

                func.call_resumable(&mut self.vm.store, &params, outputs_storage_ptr)
            }
            Some(ThreadExecution::Started(func)) => {
                let expected = {
                    let func_type = func.host_func().ty(&self.vm.store);
                    // We don't support functions with more than one result type. This should have
                    // been checked at initialization.
                    debug_assert!(func_type.results().len() <= 1);
                    func_type
                        .results()
                        .iter()
                        .next()
                        .map(|r| ValueType::try_from(*r).unwrap())
                };
                let obtained = value.as_ref().map(|v| v.ty());
                if expected != obtained {
                    return Err(RunErr::BadValueTy { expected, obtained });
                }

                let value = value.map(wasmi::Val::from);
                let inputs = match value.as_ref() {
                    Some(v) => &core::array::from_ref(v)[..],
                    None => &[],
                };

                func.resume(&mut self.vm.store, inputs, outputs_storage_ptr)
            }
            None => return Err(RunErr::Poisoned),
        };

        match result {
            Ok(wasmi::ResumableCall::Finished) => {
                // Because we have checked the signature of the function, we know that this
                // conversion can never fail.
                let thread = self.vm.threads.remove(self.index);
                let return_value = thread
                    .dummy_output_value
                    .map(|r| WasmValue::try_from(r).unwrap());
                if self.index == 0 {
                    self.vm.is_poisoned = true;
                }
                Ok(ExecOutcome::ThreadFinished {
                    return_value,
                    thread_index: self.index,
                    user_data: thread.user_data,
                })
            }
            Ok(wasmi::ResumableCall::Resumable(next)) => {
                let trap = next.host_error().downcast_ref::<InterruptedTrap>().unwrap();
                let function_index = trap.function_index;
                let params = trap.parameters.clone();
                thread_state.execution = Some(ThreadExecution::Started(next));
                Ok(ExecOutcome::Interrupted {
                    thread: self,
                    id: function_index,
                    params,
                })
            }
            Err(err) => {
                self.vm.is_poisoned = true;
                Ok(ExecOutcome::Errored {
                    thread: self,
                    error: Trap {
                        error: err.to_string(),
                    },
                })
            }
        }
    }

    /// Returns the index of the thread, so that you can retreive the thread later by calling
    /// [`ProcessStateMachine::thread`].
    ///
    /// Keep in mind that when a thread finishes, all the indices above its index shift by one.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the user data associated to that thread.
    pub fn user_data(&mut self) -> &mut T {
        &mut self.vm.threads[self.index].user_data
    }

    /// Turns this thread into the user data associated to it.
    pub fn into_user_data(self) -> &'a mut T {
        &mut self.vm.threads[self.index].user_data
    }
}

impl<'a, T> fmt::Debug for Thread<'a, T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.vm.threads[self.index], f)
    }
}

/// This dummy struct is meant to be converted to a `wasmi::core::Trap` and then back, similar to
/// `std::any::Any`.
#[derive(Debug, Clone)]
struct InterruptedTrap {
    function_index: usize,
    parameters: Vec<WasmValue>,
}

impl fmt::Display for InterruptedTrap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Interrupted")
    }
}

impl wasmi::core::HostError for InterruptedTrap {}

#[cfg(test)]
mod tests {
    use super::{ExecOutcome, NewErr, ProcessStateMachine};
    use crate::primitives::WasmValue;

    #[test]
    fn starts_if_main() {
        let module = from_wat!(
            local,
            r#"(module
            (func $_start (result i32)
                i32.const 5)
            (export "_start" (func $_start)))
        "#
        );

        let _state_machine =
            ProcessStateMachine::new(&module, (), |_, _, _| unreachable!()).unwrap();
    }

    #[test]
    fn error_if_no_main() {
        let module = from_wat!(
            local,
            r#"(module
            (func $_start (result i32)
                i32.const 5)
            (export "foo" (func $_start)))
        "#
        );

        match ProcessStateMachine::new(&module, (), |_, _, _| unreachable!()) {
            Err(NewErr::BadStartFunction) => {}
            _ => panic!(),
        }
    }

    #[test]
    fn main_executes() {
        let module = from_wat!(
            local,
            r#"(module
            (func $_start (result i32)
                i32.const 5)
            (export "_start" (func $_start)))
        "#
        );

        let mut state_machine =
            ProcessStateMachine::new(&module, (), |_, _, _| unreachable!()).unwrap();
        match state_machine.thread(0).unwrap().run(None) {
            Ok(ExecOutcome::ThreadFinished {
                return_value: Some(WasmValue::I32(5)),
                ..
            }) => {}
            _ => panic!(),
        }
        assert!(state_machine.thread(0).is_none());
    }

    #[test]
    fn external_call_then_resume() {
        let module = from_wat!(
            local,
            r#"(module
            (import "" "test" (func $test (result i32)))
            (func $_start (result i32)
                call $test)
            (export "_start" (func $_start)))
        "#
        );

        let mut state_machine = ProcessStateMachine::new(&module, (), |_, _, _| Ok(9876)).unwrap();
        match state_machine.thread(0).unwrap().run(None) {
            Ok(ExecOutcome::Interrupted {
                id: 9876,
                ref params,
                ..
            }) if params.is_empty() => {}
            _ => panic!(),
        }

        match state_machine
            .thread(0)
            .unwrap()
            .run(Some(WasmValue::I32(2227)))
        {
            Ok(ExecOutcome::ThreadFinished {
                return_value: Some(WasmValue::I32(2227)),
                ..
            }) => {}
            _ => panic!(),
        }
        assert!(state_machine.thread(0).is_none());
    }

    #[test]
    fn poisoning_works() {
        let module = from_wat!(
            local,
            r#"(module
            (func $_start
                unreachable)
            (export "_start" (func $_start)))
        "#
        );

        let mut state_machine =
            ProcessStateMachine::new(&module, (), |_, _, _| unreachable!()).unwrap();
        match state_machine.thread(0).unwrap().run(None) {
            Ok(ExecOutcome::Errored { .. }) => {}
            _ => panic!(),
        }

        assert!(state_machine.is_poisoned());

        // TODO: start running another function and check that `Poisoned` error is returned
    }

    // TODO: start mutiple threads
}
