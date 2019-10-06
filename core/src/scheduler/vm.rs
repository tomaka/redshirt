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

use crate::module::Module;
use alloc::{borrow::Cow, boxed::Box, format, vec::Vec};
use core::{cell::RefCell, convert::TryInto, fmt};
use err_derive::*;
use smallvec::SmallVec;

/// WASMI state machine dedicated to a process.
///
/// # Initialization
///
/// Initializing a state machine is done by passing a [`Module`](crate::module::Module) object,
/// which holds a successfully-parsed WASM binary.
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
    /// Original module, with resolved imports.
    module: wasmi::ModuleRef,

    /// Memory of the module instantiation.
    ///
    /// Right now we only support one unique `Memory` object per process. This is it.
    /// Contains `None` if the process doesn't export any memory object, which means it doesn't use
    /// any memory.
    memory: Option<wasmi::MemoryRef>,

    /// Table of the indirect function calls.
    ///
    /// In WASM, function pointers are in reality indices in a table called
    /// `__indirect_function_table`. This is this table, if it exists.
    indirect_table: Option<wasmi::TableRef>,

    /// List of threads that this process is running.
    threads: SmallVec<[ThreadState<T>; 4]>,

    /// If true, the state machine is in a poisoned state and cannot run any code anymore.
    is_poisoned: bool,
}

/// State of a single thread within the VM.
struct ThreadState<T> {
    /// Execution context of this thread. This notably holds the program counter, state of the
    /// stack, and so on.
    ///
    /// This field is an `Option` because we need to be able to temporarily extract it. It must
    /// always be `Some`.
    execution: Option<wasmi::FuncInvocation<'static>>,

    /// If false, then one must call `execution.start_execution()` instead of `resume_execution()`.
    /// This is a particularity of the WASM interpreter that we don't want to expose in our API.
    interrupted: bool,

    /// Opaque user data associated with the thread.
    user_data: T,
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
        return_value: Option<wasmi::RuntimeValue>,

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
        params: Vec<wasmi::RuntimeValue>,
    },

    /// The currently-executed function has finished with an error. The state machine is now in a
    /// poisoned state.
    ///
    /// Calling [`is_poisoned`](ProcessStateMachine::is_poisoned) will return true.
    Errored {
        /// Thread that error'd.
        thread: Thread<'a, T>,

        /// Error that happened.
        // TODO: error type should change here
        error: wasmi::Trap,
    },
}

/// Error that can happen when initializing a VM.
#[derive(Debug, Error)]
pub enum NewErr {
    /// Error in the interpreter.
    #[error(display = "Error in the interpreter")]
    Interpreter(#[error(cause)] wasmi::Error),
    /// The "start" symbol doesn't exist.
    #[error(display = "The \"start\" symbol doesn't exist")]
    StartNotFound,
    /// The "start" symbol must be a function.
    #[error(display = "The \"start\" symbol must be a function")]
    StartIsntAFunction,
    /// If a "memory" symbol is provided, it must be a memory.
    #[error(display = "If a \"memory\" symbol is provided, it must be a memory")]
    MemoryIsntMemory,
    /// If a "__indirect_function_table" symbol is provided, it must be a table.
    #[error(display = "If a \"__indirect_function_table\" symbol is provided, it must be a table")]
    IndirectTableIsntTable,
}

/// Error that can happen when starting a new thread.
#[derive(Debug, Error)]
pub enum StartErr {
    /// The state machine is poisoned and cannot run anymore.
    #[error(display = "State machine is in a poisoned state")]
    Poisoned,
    /// Couldn't find the requested function.
    #[error(display = "Function to start was not found")]
    FunctionNotFound,
    /// The requested function has been found in the list of exports, but it is not a function.
    #[error(display = "Symbol to start is not a function")]
    NotAFunction,
}

/// Error that can happen when resuming the execution of a function.
#[derive(Debug, Error)]
pub enum RunErr {
    /// The state machine is poisoned.
    #[error(display = "State machine is poisoned")]
    Poisoned,
    /// Passed a wrong value back.
    #[error(
        display = "Expected value of type {:?} but got {:?} instead",
        expected,
        obtained
    )]
    BadValueTy {
        /// Type of the value that was expected.
        expected: Option<wasmi::ValueType>,
        /// Type of the value that was actually passed.
        obtained: Option<wasmi::ValueType>,
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
        module: &Module,
        main_thread_user_data: T,
        mut symbols: impl FnMut(&str, &str, &wasmi::Signature) -> Result<usize, ()>,
    ) -> Result<Self, NewErr> {
        struct ImportResolve<'a>(
            RefCell<&'a mut dyn FnMut(&str, &str, &wasmi::Signature) -> Result<usize, ()>>,
        );
        impl<'a> wasmi::ImportResolver for ImportResolve<'a> {
            fn resolve_func(
                &self,
                module_name: &str,
                field_name: &str,
                signature: &wasmi::Signature,
            ) -> Result<wasmi::FuncRef, wasmi::Error> {
                let closure = &mut **self.0.borrow_mut();
                let index = match closure(module_name, field_name, signature) {
                    Ok(i) => i,
                    Err(_) => {
                        return Err(wasmi::Error::Instantiation(format!(
                            "Couldn't resolve `{}`:`{}`",
                            module_name, field_name
                        )))
                    }
                };

                Ok(wasmi::FuncInstance::alloc_host(signature.clone(), index))
            }

            fn resolve_global(
                &self,
                _module_name: &str,
                _field_name: &str,
                _global_type: &wasmi::GlobalDescriptor,
            ) -> Result<wasmi::GlobalRef, wasmi::Error> {
                Err(wasmi::Error::Instantiation(
                    "Importing globals is not supported yet".to_owned(),
                ))
            }

            fn resolve_memory(
                &self,
                _module_name: &str,
                _field_name: &str,
                _memory_type: &wasmi::MemoryDescriptor,
            ) -> Result<wasmi::MemoryRef, wasmi::Error> {
                Err(wasmi::Error::Instantiation(
                    "Importing memory is not supported yet".to_owned(),
                ))
            }

            fn resolve_table(
                &self,
                _module_name: &str,
                _field_name: &str,
                _table_type: &wasmi::TableDescriptor,
            ) -> Result<wasmi::TableRef, wasmi::Error> {
                Err(wasmi::Error::Instantiation(
                    "Importing tables is not supported yet".to_owned(),
                ))
            }
        }

        let not_started =
            wasmi::ModuleInstance::new(module.as_ref(), &ImportResolve(RefCell::new(&mut symbols)))
                .map_err(NewErr::Interpreter)?;

        // TODO: WASM has a special "start" instruction that can be used to designate a function
        // that must be executed before the module is considered initialized. It is unclear whether
        // this is intended to be a function that for example initializes global variables, or if
        // this is an equivalent of "_start". In practice, Rust never seems to generate such as
        // "start" instruction, so for now we ignore it. The code below panics if there is such
        // a "start" item, so we will fortunately not blindly run into troubles.
        let module = not_started.assert_no_start();

        let memory = if let Some(mem) = module.export_by_name("memory") {
            if let Some(mem) = mem.as_memory() {
                Some(mem.clone())
            } else {
                return Err(NewErr::MemoryIsntMemory);
            }
        } else {
            None
        };

        let indirect_table = if let Some(tbl) = module.export_by_name("__indirect_function_table") {
            if let Some(tbl) = tbl.as_table() {
                Some(tbl.clone())
            } else {
                return Err(NewErr::IndirectTableIsntTable);
            }
        } else {
            None
        };

        let mut state_machine = ProcessStateMachine {
            module,
            memory,
            indirect_table,
            is_poisoned: false,
            threads: SmallVec::new(),
        };

        // Try to start executing `_start`.
        match state_machine.start_thread_by_name("_start", &[][..], main_thread_user_data) {
            Ok(_) => {}
            Err(StartErr::FunctionNotFound) => return Err(NewErr::StartNotFound),
            Err(StartErr::Poisoned) => unreachable!(),
            Err(StartErr::NotAFunction) => return Err(NewErr::StartIsntAFunction),
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
    pub fn start_thread_by_id(
        &mut self,
        function_id: u32,
        params: impl Into<Cow<'static, [wasmi::RuntimeValue]>>,
        user_data: T,
    ) -> Result<Thread<T>, StartErr> {
        if self.is_poisoned {
            return Err(StartErr::Poisoned);
        }

        // Find the function within the process.
        let function = self
            .indirect_table
            .as_ref()
            .and_then(|t| t.get(function_id).ok())
            .and_then(|f| f)
            .ok_or(StartErr::FunctionNotFound)?;

        let execution = wasmi::FuncInstance::invoke_resumable(&function, params).unwrap();
        self.threads.push(ThreadState {
            execution: Some(execution),
            interrupted: false,
            user_data,
        });

        let thread_id = self.threads.len() - 1;
        Ok(Thread {
            vm: self,
            index: thread_id,
        })
    }

    /// Same as [`start_thread_by_id`](ProcessStateMachine::start_thread_by_id), but executes a
    /// symbol by name.
    fn start_thread_by_name(
        &mut self,
        symbol_name: &str,
        params: impl Into<Cow<'static, [wasmi::RuntimeValue]>>,
        user_data: T,
    ) -> Result<Thread<T>, StartErr> {
        if self.is_poisoned {
            return Err(StartErr::Poisoned);
        }

        match self.module.export_by_name(symbol_name) {
            Some(wasmi::ExternVal::Func(f)) => {
                let execution = wasmi::FuncInstance::invoke_resumable(&f, params).unwrap();
                self.threads.push(ThreadState {
                    execution: Some(execution),
                    interrupted: false,
                    user_data,
                });
            }
            None => return Err(StartErr::FunctionNotFound),
            _ => return Err(StartErr::NotAFunction),
        }

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
    pub fn read_memory(&self, offset: u32, size: u32) -> Result<Vec<u8>, ()> {
        self.memory
            .as_ref()
            .unwrap()
            .get(offset, size.try_into().map_err(|_| ())?)
            .map_err(|_| ())
    }

    /// Write the data at the given memory location.
    ///
    /// Returns an error if the range is invalid or out of range.
    pub fn write_memory(&mut self, offset: u32, value: &[u8]) -> Result<(), ()> {
        self.memory
            .as_ref()
            .unwrap()
            .set(offset, value)
            .map_err(|_| ())
    }
}

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
    pub fn run(mut self, value: Option<wasmi::RuntimeValue>) -> Result<ExecOutcome<'a, T>, RunErr> {
        struct DummyExternals;
        impl wasmi::Externals for DummyExternals {
            fn invoke_index(
                &mut self,
                index: usize,
                args: wasmi::RuntimeArgs,
            ) -> Result<Option<wasmi::RuntimeValue>, wasmi::Trap> {
                Err(wasmi::TrapKind::Host(Box::new(Interrupt {
                    index,
                    args: args.as_ref().to_vec(),
                }))
                .into())
            }
        }

        #[derive(Debug)]
        struct Interrupt {
            index: usize,
            args: Vec<wasmi::RuntimeValue>,
        }
        impl fmt::Display for Interrupt {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "Interrupt")
            }
        }
        impl wasmi::HostError for Interrupt {}

        if self.vm.is_poisoned {
            return Err(RunErr::Poisoned);
        }

        let thread_state = &mut self.vm.threads[self.index];

        let mut execution = thread_state.execution.take().unwrap();
        let result = if thread_state.interrupted {
            let expected_ty = execution.resumable_value_type();
            let obtained_ty = value.as_ref().map(|v| v.value_type());
            if expected_ty != obtained_ty {
                return Err(RunErr::BadValueTy {
                    expected: expected_ty,
                    obtained: obtained_ty,
                });
            }
            execution.resume_execution(value, &mut DummyExternals)
        } else {
            if value.is_some() {
                return Err(RunErr::BadValueTy {
                    expected: None,
                    obtained: value.as_ref().map(|v| v.value_type()),
                });
            }
            thread_state.interrupted = true;
            execution.start_execution(&mut DummyExternals)
        };

        match result {
            Ok(return_value) => {
                let user_data = self.vm.threads.remove(self.index).user_data;
                // If this is the "main" function, the state machine is now poisoned.
                if self.index == 0 {
                    self.vm.is_poisoned = true;
                }
                Ok(ExecOutcome::ThreadFinished {
                    thread_index: self.index,
                    return_value,
                    user_data,
                })
            }
            Err(wasmi::ResumableError::AlreadyStarted) => unreachable!(),
            Err(wasmi::ResumableError::NotResumable) => unreachable!(),
            Err(wasmi::ResumableError::Trap(ref trap)) if trap.kind().is_host() => {
                let interrupt: &Interrupt = match trap.kind() {
                    wasmi::TrapKind::Host(err) => err.downcast_ref().unwrap(),
                    _ => unreachable!(),
                };
                thread_state.execution = Some(execution);
                Ok(ExecOutcome::Interrupted {
                    thread: self,
                    id: interrupt.index,
                    params: interrupt.args.clone(),
                })
            }
            Err(wasmi::ResumableError::Trap(trap)) => {
                self.vm.is_poisoned = true;
                Ok(ExecOutcome::Errored {
                    thread: self,
                    error: trap,
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

#[cfg(test)]
mod tests {
    use super::{ExecOutcome, NewErr, ProcessStateMachine};
    use crate::module::Module;

    #[test]
    fn start_in_paused_if_main() {
        let module = Module::from_wat(
            r#"(module
            (func $_start (result i32)
                i32.const 5)
            (export "_start" (func $_start)))
        "#,
        )
        .unwrap();

        let _state_machine = ProcessStateMachine::new(&module, (), |_, _, _| unreachable!()).unwrap();
    }

    #[test]
    fn start_stopped_if_no_main() {
        let module = Module::from_wat(
            r#"(module
            (func $_start (result i32)
                i32.const 5)
            (export "foo" (func $_start)))
        "#,
        )
        .unwrap();

        match ProcessStateMachine::new(&module, (), |_, _, _| unreachable!()) {
            Err(NewErr::StartNotFound) => {}
            _ => panic!(),
        }
    }

    #[test]
    fn main_executes() {
        let module = Module::from_wat(
            r#"(module
            (func $_start (result i32)
                i32.const 5)
            (export "_start" (func $_start)))
        "#,
        )
        .unwrap();

        let mut state_machine =
            ProcessStateMachine::new(&module, (), |_, _, _| unreachable!()).unwrap();
        match state_machine.thread(0).unwrap().run(None) {
            Ok(ExecOutcome::ThreadFinished { return_value: Some(wasmi::RuntimeValue::I32(5)), .. }) => {}
            _ => panic!(),
        }
        assert!(state_machine.thread(0).is_none());
    }

    #[test]
    fn external_call_then_resume() {
        let module = Module::from_wat(
            r#"(module
            (import "" "test" (func $test (result i32)))
            (func $_start (result i32)
                call $test)
            (export "_start" (func $_start)))
        "#,
        )
        .unwrap();

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
            .run(Some(wasmi::RuntimeValue::I32(2227)))
        {
            Ok(ExecOutcome::ThreadFinished { return_value: Some(wasmi::RuntimeValue::I32(2227)), .. }) => {}
            _ => panic!(),
        }
        assert!(state_machine.thread(0).is_none());
    }

    #[test]
    fn poisoning_works() {
        let module = Module::from_wat(
            r#"(module
            (func $_start
                unreachable)
            (export "_start" (func $_start)))
        "#,
        )
        .unwrap();

        let mut state_machine =
            ProcessStateMachine::new(&module, (), |_, _, _| unreachable!()).unwrap();
        match state_machine.thread(0).unwrap().run(None) {
            Ok(ExecOutcome::Errored { .. }) => {}
            _ => panic!(),
        }

        assert!(state_machine.is_poisoned());

        // TODO: start running another function and check that `Poisoned` error is returned
    }
}
