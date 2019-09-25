// Copyright(c) 2019 Pierre Krieger

use crate::interface::{InterfaceHash, InterfaceId};
use crate::module::Module;
use alloc::borrow::Cow;
use core::{cell::RefCell, fmt, ops::Bound, ops::RangeBounds};
use err_derive::*;
use smallvec::SmallVec;

/// WASMI state machine dedicated to a process.
///
/// # Initialization
///
/// Initializing a state machine is done by passing a [`Module`](crate::module::Module) object,
/// which holds a successfully-parsed WASM binary.
///
/// The module might contain a list of functions to import that the initialization process must
/// resolve. When such an import is encountered, the closure passed to the
/// [`new`](ProcessStateMachine::new) function is invoked and must return an opaque integer decided
/// by the user. This integer is later passed back to the user of this struct in situations when
/// the state machine invokes that external function.
///
/// # Threads
///
/// This struct is composed of one or multiple threads. When initialized, the VM starts with a
/// single thread at the start of the "main" function of the WASM module.
///
/// In order to run the VM, grab a thread by calling [`ProcessStateMachine::threads`], then call
/// [`Thread::run`]. The thread will then run until it either finishes (in which case the thread
/// is then destroyed), or attempts to call an imported function. The [`run`](Thread::run) method
/// requires passing a value to inject back and that corresponds to the return value obtained by
/// executing the imported function. When a thread is created, this inject-back value must be
/// `None`.
///
/// The generic parameter of this struct is some userdata that is associated with each thread.
/// You must pass a value when creating a thread, and can retreive it later.
///
/// # Poisonning
/// 
/// If the main thread stops, or if any thread encounters an error, then the VM moves into a
/// "poisoned" state. It is then no longer possible to run anything in it. If the main thread
/// stops, then all threads are destroyed.
///
/// # Single-threaded-ness
///
/// The [`ProcessStateMachine`] is single-threaded. In other words, the VM can only ever run one
/// thread simultaneously. This might change in the future.
/// 
// TODO: Debug
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
// TODO: Debug
struct ThreadState<T> {
    /// Each program can only run once at a time. It only has one "thread".
    /// If `Some`, we are currently executing something in `Program`. If `None`, we aren't.
    execution: Option<wasmi::FuncInvocation<'static>>,

    /// If false, then one must call `execution.start_execution()` instead of `resume_execution()`.
    /// This is a special situation that is required after we put a value in `execution`.
    interrupted: bool,

    /// Opaque user data associated to the thread.
    user_data: T,
}

/// Access to a thread within the virtual machine.
// TODO: Debug
pub struct Thread<'a, T> {
    /// Reference to the parent object.
    vm: &'a mut ProcessStateMachine<T>,

    // Index within [`ProcessStateMachine::threads`] of the thread we are referencing.
    index: usize,
}

/// Outcome of the [`run`](Thread::run) function.
// TODO: restore: #[derive(Debug)]
pub enum ExecOutcome<'a, T> {
    /// A thread has finished. The thread no longer exists.
    ///
    /// If this was the main thread, calling [`is_poisoned`](ProcessStateMachine::is_poisoned)
    /// will return true.
    // TODO: return all the user datas of all other threads if this is the main thread?
    //       or have a different enum variant?
    Finished {
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
        /// Error that happened.
        // TODO: error type should change here
        error: wasmi::Trap,
        /// User data that was associated to thread.
        user_data: T,
    },
}

/// Error that can happen when initializing a VM.
#[derive(Debug, Error)]
pub enum NewErr {
    /// Error in the interpreter.
    #[error(display = "Error in the interpreter")]
    Interpreter(#[error(cause)] wasmi::Error),
    /// The "main" symbol doesn't exist.
    #[error(display = "The \"main\" symbol doesn't exist")]
    MainNotFound,
    /// The "main" symbol must be a function.
    #[error(display = "The \"main\" symbol must be a function")]
    MainIsntAFunction,
    /// If a "memory" symbol is provided, it must be a memory.
    #[error(display = "If a \"memory\" symbol is provided, it must be a memory")]
    MemoryIsntMemory,
    /// If a "__indirect_function_table" symbol is provided, it must be a table.
    #[error(display = "If a \"__indirect_function_table\" symbol is provided, it must be a table")]
    IndirectTableIsntTable,
}

/// Error that can happen when starting the execution of a function.
#[derive(Debug, Error)]
pub enum StartErr {
    /// The state machine is poisoned and cannot run anymore.
    #[error(display = "State machine is in a poisoned state")]
    Poisoned,
    /// Couldn't find the requested function.
    #[error(display = "Function to start was not found")]
    SymbolNotFound,
    /// The requested function has been found in the list of exports, but it is not a function.
    #[error(display = "Symbol to start is not a function")]
    NotAFunction,
}

/// Error that can happen when resuming the execution of a function.
#[derive(Debug, Error)]
pub enum RunErr {
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
    /// If a start function exists in the module, we start executing it and the returned object is
    /// in the paused state. If that is the case, one must call `resume` with a `None` pass-back
    /// value in order to resume execution of `main`.
    pub fn new(
        module: &Module,
        main_thread_user_data: T,
        mut symbols: impl FnMut(&InterfaceId, &str, &wasmi::Signature) -> Result<usize, ()>,
    ) -> Result<Self, NewErr> {
        struct ImportResolve<'a>(
            RefCell<&'a mut dyn FnMut(&InterfaceId, &str, &wasmi::Signature) -> Result<usize, ()>>,
        );
        impl<'a> wasmi::ImportResolver for ImportResolve<'a> {
            fn resolve_func(
                &self,
                module_name: &str,
                field_name: &str,
                signature: &wasmi::Signature,
            ) -> Result<wasmi::FuncRef, wasmi::Error> {
                // Parse `module_name` as if it is a base58 representation of an interface hash.
                let interface_hash = {
                    let mut buf_out = [0; 32];
                    let mut buf_interm = [0; 32];
                    match bs58::decode(module_name).into(&mut buf_interm[..]) {
                        Ok(n) => {
                            buf_out[(32 - n)..].copy_from_slice(&buf_interm[..n]);
                            InterfaceId::Hash(InterfaceHash::from(buf_out))
                        }
                        Err(_) => InterfaceId::Bytes(module_name.to_owned()),
                    }
                };

                let closure = &mut **self.0.borrow_mut();
                let index = match closure(&interface_hash, field_name, signature) {
                    Ok(i) => i,
                    Err(_) => {
                        return Err(wasmi::Error::Instantiation(format!(
                            "Couldn't resolve `{:?}`:`{}`",
                            interface_hash, field_name
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
        // this is an equivalent of "main". In practice, Rust never seems to generate such as
        // "start" instruction, so for now we ignore it.
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

        // Try to start executing `main`.
        match state_machine.start_thread_inner(
            "main",
            &[wasmi::RuntimeValue::I32(0), wasmi::RuntimeValue::I32(0)][..],
            main_thread_user_data,
        ) {
            Ok(_) => {}
            Err(StartErr::SymbolNotFound) => return Err(NewErr::MainNotFound),
            Err(StartErr::Poisoned) => unreachable!(),
            Err(StartErr::NotAFunction) => return Err(NewErr::MainIsntAFunction),
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
    /// Returns an error if [`is_executing`](ProcessStateMachine::is_executing) returns true.
    ///
    /// You should call [`resume`](ProcessStateMachine::resume) afterwards with a value of `None`.
    pub fn start_thread_by_id(
        &mut self,
        interface: &InterfaceHash,
        function: &str,
        params: impl Into<Cow<'static, [wasmi::RuntimeValue]>>,
        user_data: T,
    ) -> Result<(), StartErr> {
        unimplemented!()
    }

    /// Same as `start`, but executes a symbol by name.
    fn start_thread_inner(
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
            None => return Err(StartErr::SymbolNotFound),
            _ => return Err(StartErr::NotAFunction),
        }

        let thread_id = self.threads.len() - 1;
        Ok(Thread {
            vm: self,
            index: thread_id,
        })
    }

    /// Returns the number of threads that we have.
    pub fn num_threads(&self) -> usize {
        self.threads.len()
    }

    /// Returns the thread with the given index.
    ///
    /// Returns `None` if the index is superior or equal to what [`num_threads`] would return.
    pub fn thread(&mut self, index: usize) -> Option<Thread<T>> {
        if index < self.threads.len() {
            Some(Thread {
                vm: self,
                index,
            })
        } else {
            None
        }
    }

    /// Copies the given memory range into a `Vec<u8>`.
    ///
    /// Returns an error if the range is invalid or out of range.
    // TODO: should really return &mut [u8] I think
    pub fn read_memory(&self, range: impl RangeBounds<usize>) -> Result<Vec<u8>, ()> {
        // TODO: there's a method to do that in wasmi
        self.dma(range, |mem| mem.to_vec())
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

    fn dma<R>(
        &self,
        range: impl RangeBounds<usize>,
        f: impl FnOnce(&mut [u8]) -> R,
    ) -> Result<R, ()> {
        let mem = self.memory.as_ref().unwrap();
        let mem_sz = mem.current_size().0 * 65536;

        let start = match range.start_bound() {
            Bound::Included(b) => *b,
            Bound::Excluded(b) => b.checked_add(1).ok_or(())?,
            Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            Bound::Included(b) => b.checked_add(1).ok_or(())?,
            Bound::Excluded(b) => *b,
            Bound::Unbounded => mem_sz,
        };

        if start > end || end > mem_sz {
            return Err(());
        }

        Ok(mem.with_direct_access_mut(move |mem| f(&mut mem[start..end])))
    }
}

impl<'a, T> Thread<'a, T> {
    /// Resumes execution of the thread.
    ///
    /// If this is the first call you call [`run`](Thread::run) for this thread, then you must pass
    /// a value of `None`.
    ///
    /// If you call this function after a previous call to [`run`](Thread::run) that was
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

        assert!(!self.vm.is_poisoned);

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
                // If this is the "main" function, destroy all threads.
                let user_data = self.vm.threads.remove(self.index).user_data;
                if self.index == 0 {
                    self.vm.threads.clear();
                }
                Ok(ExecOutcome::Finished {
                    return_value,
                    user_data,
                })
            },
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
                let user_data = self.vm.threads.remove(self.index).user_data;
                self.vm.is_poisoned = true;
                Ok(ExecOutcome::Errored {
                    user_data,
                    error: trap,
                })
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{ExecOutcome, NewErr, ProcessStateMachine};
    use crate::module::Module;

    #[test]
    fn start_in_paused_if_main() {
        let module = Module::from_wat(
            r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                i32.const 5)
            (export "main" (func $main)))
        "#,
        )
        .unwrap();

        let _state_machine = ProcessStateMachine::new(&module, |_, _, _| unreachable!()).unwrap();
    }

    #[test]
    fn start_stopped_if_no_main() {
        let module = Module::from_wat(
            r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                i32.const 5)
            (export "foo" (func $main)))
        "#,
        )
        .unwrap();

        match ProcessStateMachine::new(&module, |_, _, _| unreachable!()) {
            Err(NewErr::MainNotFound) => {},
            _ => panic!()
        }
    }

    #[test]
    fn main_executes() {
        let module = Module::from_wat(
            r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                i32.const 5)
            (export "main" (func $main)))
        "#,
        )
        .unwrap();

        let mut state_machine =
            ProcessStateMachine::new(&module, |_, _, _| unreachable!()).unwrap();
        match state_machine.thread(0).unwrap().run(None) {
            Ok(ExecOutcome::Finished(Some(wasmi::RuntimeValue::I32(5)))) => {}
            _ => panic!(),
        }
        assert!(state_machine.thread(0).is_none());
    }

    #[test]
    fn external_call_then_resume() {
        let module = Module::from_wat(
            r#"(module
            (import "" "test" (func $test (result i32)))
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                call $test)
            (export "main" (func $main)))
        "#,
        )
        .unwrap();

        let mut state_machine = ProcessStateMachine::new(&module, |_, _, _| Ok(9876)).unwrap();
        match state_machine.thread(0).unwrap().run(None) {
            Ok(ExecOutcome::Interrupted {
                id: 9876,
                ref params,
                ..
            }) if params.is_empty() => {}
            _ => panic!(),
        }

        match state_machine.thread(0).unwrap().run(Some(wasmi::RuntimeValue::I32(2227))) {
            Ok(ExecOutcome::Finished(Some(wasmi::RuntimeValue::I32(2227)))) => {}
            _ => panic!(),
        }
        assert!(state_machine.thread(0).is_none());
    }

    #[test]
    fn poisoning_works() {
        let module = Module::from_wat(
            r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                unreachable)
            (export "main" (func $main)))
        "#,
        )
        .unwrap();

        let mut state_machine =
            ProcessStateMachine::new(&module, |_, _, _| unreachable!()).unwrap();
        match state_machine.thread(0).unwrap().run(None) {
            Ok(ExecOutcome::Errored(_)) => {}
            _ => panic!(),
        }

        assert!(state_machine.is_poisoned());

        // TODO: start running another function and check that `Poisoned` error is returned
    }

}
