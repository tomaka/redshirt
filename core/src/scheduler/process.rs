use crate::interface::{InterfaceHash, InterfaceId};
use crate::module::Module;
use alloc::borrow::Cow;
use core::{cell::RefCell, fmt};

/// WASMI state machine dedicated to a process.
///
/// # Initialization
///
/// Initializing a state machine is done by passing a [`Module`](crate::module::Module) object,
/// which holds a successfully-parsed WASM binary.
///
/// The module might contain a list of elements to import (such a functions) and that the
/// initialization process must resolve. When such an import is encountered, the closure passed
/// to the [`new`](ProcessStateMachine::new) function is invoked and must return a meaning-less
/// integer decided by the user. This integer is later passed back to the user of this struct in
/// situations when the state machine invokes that external function.
///
/// # Paused vs stopped
///
/// This struct can be in two different states: either paused, or stopped. At initialization,
/// if the WASM module has a startup function, it will immediately start running it and pause.
///
/// When the state machine is stopped, you can call [`start`](ProcessStateMachine::start) in order
/// to switch the state machine to a paused state at the start of that function.
///
/// When the state machine is paused, you can call [`resume`](ProcessStateMachine::running) in
/// order to execute code until the next pause.
///
/// The state machine immediately pauses itself if it encounters an external function call (as in,
/// a function that's been imported), in which case you must execute that call and feed back the
/// outcome of that call into the state machine to resume it.
///
/// # Shared memory
///
/// TO BE DESIGNED // TODO:
pub struct ProcessStateMachine {
    /// Original module, with resolved imports.
    module: wasmi::ModuleRef,

    /// Each program can only run once at a time. It only has one "thread".
    /// If `Some`, we are currently executing something in `Program`. If `None`, we aren't.
    execution: Option<wasmi::FuncInvocation<'static>>,

    /// If false, then one must call `execution.start_execution()` instead of `resume_execution()`.
    /// This is a special situation that is required after we put a value in `execution`.
    interrupted: bool,
}

impl ProcessStateMachine {
    /// Creates a new process state machine from the given module.
    ///
    /// The closure is called for each import that the module has.
    ///
    /// If a start function exists in the module, we start executing it and the returned object is
    /// in the paused state. If that is the case, one must call `resume` with a `None` pass-back
    /// value in order to resume execution of `main`.
    pub fn new(module: &Module, mut symbols: impl FnMut(&InterfaceId, &str, &wasmi::Signature) -> Result<usize, ()>) -> Result<Self, ()> {
        struct ImportResolve<'a>(RefCell<&'a mut dyn FnMut(&InterfaceId, &str, &wasmi::Signature) -> Result<usize, ()>>);
        impl<'a> wasmi::ImportResolver for ImportResolve<'a> {
            fn resolve_func(&self, module_name: &str, field_name: &str, signature: &wasmi::Signature)
                -> Result<wasmi::FuncRef, wasmi::Error>
            {
                // Parse `module_name` as if it is a base58 representation of an interface hash.
                let interface_hash = {
                    let mut buf_out = [0; 32];
                    let mut buf_interm = [0; 32];
                    match bs58::decode(module_name).into(&mut buf_interm[..]) {
                        Ok(n) => {
                            buf_out[(32 - n)..].copy_from_slice(&buf_interm[..n]);
                            InterfaceId::Hash(InterfaceHash::from(buf_out))
                        },
                        Err(_) => InterfaceId::Bytes(module_name.to_owned()),
                    }
                };

                let closure = &mut **self.0.borrow_mut();
                let index = match closure(&interface_hash, field_name, signature) {
                    Ok(i) => i,
                    Err(_) => return Err(wasmi::Error::Instantiation(format!("Couldn't resolve `{:?}`:`{}`", interface_hash, field_name))),
                };

                Ok(wasmi::FuncInstance::alloc_host(signature.clone(), index))
            }

            fn resolve_global(&self, _module_name: &str, _field_name: &str, _global_type: &wasmi::GlobalDescriptor)
                -> Result<wasmi::GlobalRef, wasmi::Error>
            {
                Err(wasmi::Error::Instantiation("Importing globals is not supported yet".to_owned()))
            }

            fn resolve_memory(&self, _module_name: &str, _field_name: &str, _memory_type: &wasmi::MemoryDescriptor)
                -> Result<wasmi::MemoryRef, wasmi::Error>
            {
                Err(wasmi::Error::Instantiation("Importing memory is not supported yet".to_owned()))
            }

            fn resolve_table(&self, _module_name: &str, _field_name: &str, _table_type: &wasmi::TableDescriptor)
                -> Result<wasmi::TableRef, wasmi::Error>
            {
                Err(wasmi::Error::Instantiation("Importing tables is not supported yet".to_owned()))
            }
        }

        let not_started = wasmi::ModuleInstance::new(module.as_ref(), &ImportResolve(RefCell::new(&mut symbols))).unwrap();      // TODO: don't unwrap
        let module = not_started.assert_no_start();     // TODO: true in practice, bad to do in theory

        let mut state_machine = ProcessStateMachine {
            module,
            execution: None,
            interrupted: false,
        };
        // Try to start executing `main`.
        let _ = state_machine.start_inner("main", &[wasmi::RuntimeValue::I32(0), wasmi::RuntimeValue::I32(0)][..]);
        Ok(state_machine)
    }

    /// Returns true if we are executing something and are in the paused state.
    ///
    /// If false, we are stopped.
    pub fn is_executing(&self) -> bool {
        self.execution.is_some()
    }

    /// Starts executing a function. Immediately pauses the execution and puts it in an
    /// interrupted state.
    ///
    /// Only valid to call if [`is_executing`](ProcessStateMachine::is_executing) returns false.
    ///
    /// You should call [`resume`](ProcessStateMachine::resume) afterwards with a value of `None`.
    pub fn start(&mut self, interface: &InterfaceHash, function: &str, params: impl Into<Cow<'static, [wasmi::RuntimeValue]>>) {

        unimplemented!()
    }

    /// Same as `start`, but executes a symbol by name.
    fn start_inner(&mut self, symbol_name: &str, params: impl Into<Cow<'static, [wasmi::RuntimeValue]>>) {
        if self.execution.is_some() {
            panic!()        // TODO: return error
        }

        match self.module.export_by_name(symbol_name) {
            Some(wasmi::ExternVal::Func(f)) => {
                let execution = wasmi::FuncInstance::invoke_resumable(&f, params).unwrap();
                self.execution = Some(execution);
                self.interrupted = false;
            },
            None => panic!(),   // TODO: error
            _ => panic!()       // TODO:
        }
    }

    /// Resumes execution when in a paused state.
    ///
    /// If this is the first call you call [`resume`](ProcessStateMachine::resume) after a call to
    /// [`start`](ProcessStateMachine::start) or to [`new`](ProcessStateMachine::new), then you
    /// must pass a value of `None`.
    ///
    /// If you call this function after a previous call to [`resume`](ProcessStateMachine::resume)
    /// that was interrupted by an external function call, then you must pass back the outcome of
    /// that call.
    ///
    /// Only valid to call if [`is_executing`](ProcessStateMachine::is_executing) returns true.
    pub fn resume(&mut self, value: Option<wasmi::RuntimeValue>) -> ExecOutcome {
        struct DummyExternals;
        impl wasmi::Externals for DummyExternals {
            fn invoke_index(&mut self, index: usize, args: wasmi::RuntimeArgs)
                -> Result<Option<wasmi::RuntimeValue>, wasmi::Trap>
            {
                Err(wasmi::TrapKind::Host(Box::new(Interrupt { index, args: args.as_ref().to_vec() })).into())
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
        impl wasmi::HostError for Interrupt {
        }

        let mut execution = self.execution.take().unwrap();
        let result = if self.interrupted {
            debug_assert_eq!(
                execution.resumable_value_type(),
                value.as_ref().map(|v| v.value_type())
            );
            execution.resume_execution(value, &mut DummyExternals)
        } else {
            assert!(value.is_none());       // TODO: turn into an error
            self.interrupted = true;
            execution.start_execution(&mut DummyExternals)
        };

        match result {
            Ok(val) => ExecOutcome::Finished(val),
            Err(wasmi::ResumableError::AlreadyStarted) => unreachable!(),
            Err(wasmi::ResumableError::NotResumable) => unreachable!(),
            Err(wasmi::ResumableError::Trap(ref trap)) if trap.kind().is_host() => {
                let interrupt: &Interrupt = match trap.kind() {
                    wasmi::TrapKind::Host(err) => err.downcast_ref().unwrap(),
                    _ => unreachable!()
                };
                self.execution = Some(execution);
                ExecOutcome::Interrupted {
                    id: interrupt.index,
                    params: interrupt.args.clone(),
                }
            }
            Err(wasmi::ResumableError::Trap(trap)) => {
                println!("oops, actual error!");
                // TODO: put in corrupted state?
                ExecOutcome::Errored(trap)
            }
        }
    }
}

/// Outcome of the [`resume`](ProcessStateMachine::resume) function.
pub enum ExecOutcome {
    /// The currently-executed function has finished. The state machine is now in a stopped state.
    ///
    /// Calling [`is_executing`](ProcessStateMachine::is_executing) will return false.
    Finished(Option<wasmi::RuntimeValue>),

    /// The currently-executed function has been paused due to a call to an external function.
    /// The state machine is now in a paused state.
    ///
    /// Calling [`is_executing`](ProcessStateMachine::is_executing) will return true.
    ///
    /// This variant contains the identifier of the external function that is expected to be
    /// called, and its parameters. When you call [`resume`](ProcessStateMachine::resume) again,
    /// you must pass back the outcome of calling that function.
    ///
    /// > **Note**: The type of the return value of the function is called is not specified, as the
    /// >           user is supposed to know it based on the identifier. It is an error tp call
    /// >           [`resume`](ProcessStateMachine::resume) with a value of the wrong type.
    Interrupted {
        /// Identifier of the function to call. Corresponds to the value provided at
        /// initialization when resolving imports.
        id: usize,

        /// Parameters of the function call.
        params: Vec<wasmi::RuntimeValue>,
    },

    /// The currently-executed function has finished with an error. The state machine is now in a
    /// stopped state.
    ///
    /// Calling [`is_executing`](ProcessStateMachine::is_executing) will return false.
    // TODO: it might change a bit here
    Errored(wasmi::Trap),
}

#[cfg(test)]
mod tests {
    use crate::module::Module;
    use super::{ProcessStateMachine, ExecOutcome};

    #[test]
    fn start_in_paused_if_main() {
        let module = Module::from_wat(r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                i32.const 5)
            (export "main" (func $main)))
        "#).unwrap();

        let state_machine = ProcessStateMachine::new(&module, |_, _, _| unreachable!()).unwrap();
        assert!(state_machine.is_executing());  // there is "main" function
    }

    #[test]
    #[ignore]       // TODO: panics as not implemented
    fn start_stopped_if_no_main() {
        let module = Module::from_wat(r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                i32.const 5)
            (export "foo" (func $main)))
        "#).unwrap();

        let state_machine = ProcessStateMachine::new(&module, |_, _, _| unreachable!()).unwrap();
        assert!(!state_machine.is_executing());
    }

    #[test]
    fn main_executes() {
        let module = Module::from_wat(r#"(module
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                i32.const 5)
            (export "main" (func $main)))
        "#).unwrap();

        let mut state_machine = ProcessStateMachine::new(&module, |_, _, _| unreachable!()).unwrap();
        match state_machine.resume(None) {
            ExecOutcome::Finished(Some(wasmi::RuntimeValue::I32(5))) => {}
            _ => panic!()
        }
        assert!(!state_machine.is_executing());
    }

    #[test]
    fn external_call_then_resume() {
        let module = Module::from_wat(r#"(module
            (import "" "test" (func $test (result i32)))
            (func $main (param $p0 i32) (param $p1 i32) (result i32)
                call $test)
            (export "main" (func $main)))
        "#).unwrap();

        let mut state_machine = ProcessStateMachine::new(&module, |_, _, _| Ok(9876)).unwrap();
        match state_machine.resume(None) {
            ExecOutcome::Interrupted { id: 9876, ref params } if params.is_empty() => {}
            _ => panic!()
        }
        assert!(state_machine.is_executing());

        match state_machine.resume(Some(wasmi::RuntimeValue::I32(2227))) {
            ExecOutcome::Finished(Some(wasmi::RuntimeValue::I32(2227))) => {}
            _ => panic!()
        }
        assert!(!state_machine.is_executing());
    }
}
