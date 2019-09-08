use crate::interface::InterfaceHash;
use crate::module::Module;
use core::{cell::RefCell, fmt};

pub struct ProcessStateMachine {
    module: wasmi::ModuleRef,       // TODO: ask serguey or someone whether that's a weak ref

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
    /// in the executing state. If that is the case, one must call `resume` with a `None` pass-back
    /// value in order to resume execution of `main`.
    pub fn new(module: &Module, mut symbols: impl FnMut(&InterfaceHash, &str) -> Result<usize, ()>) -> Result<Self, ()> {
        struct ImportResolve<'a>(RefCell<&'a mut dyn FnMut(&InterfaceHash, &str) -> Result<usize, ()>>);
        impl<'a> wasmi::ImportResolver for ImportResolve<'a> {
            fn resolve_func(&self, module_name: &str, field_name: &str, signature: &wasmi::Signature)
                -> Result<wasmi::FuncRef, wasmi::Error>
            {
                // Parse `module_name` as if it is a base58 representation of an interface hash.
                let interface_hash = {
                    let mut buf_out = [0; 32];
                    let mut buf_interm = [0; 32];
                    match bs58::decode(module_name).into(&mut buf_interm[..]) {
                        Ok(n) => buf_out[(32 - n)..].copy_from_slice(&buf_interm[..n]),
                        Err(err) => return Err(wasmi::Error::Instantiation(format!("Error while decoding module name `{}`: {}", module_name, err))),
                    }
                    InterfaceHash::from(buf_out)
                };

                println!("{:?}", interface_hash);

                let closure = &mut **self.0.borrow_mut();
                let index = match closure(&interface_hash, field_name) {
                    Ok(i) => i,
                    Err(_) => return Err(wasmi::Error::Instantiation(format!("Couldn't resolve `{}`:`{}`", interface_hash, field_name))),
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

        let main_execution = match module.export_by_name("main") {
            Some(wasmi::ExternVal::Func(f)) => {
                let execution = wasmi::FuncInstance::invoke_resumable(&f, &[wasmi::RuntimeValue::I32(0), wasmi::RuntimeValue::I32(0)][..]).unwrap();
                Some(execution)
            },
            None => None,
            _ => panic!()       // TODO:
        };

        Ok(ProcessStateMachine {
            module,
            execution: main_execution,
            interrupted: false,
        })
    }

    /// Returns true if we are executing something.
    pub fn is_executing(&self) -> bool {
        self.execution.is_some()
    }

    /// Starts executing a function. Immediately pauses the execution and puts it in an
    /// interrupted state.
    ///
    /// Only valid to call if `is_executing` is false.
    ///
    /// Call `resume` afterwards with a value of `None`.
    pub fn start(&mut self, interface: &InterfaceHash, function: &str) {
        unimplemented!()
    }

    /// Only valid to call if `is_executing` is true.
    pub fn resume(&mut self, value: Option<wasmi::RuntimeValue>) -> ExecOutcome {
        let mut execution = self.execution.take().unwrap();
        let result = if self.interrupted {
            debug_assert_eq!(
                execution.resumable_value_type(),
                value.as_ref().map(|v| v.value_type())
            );
            execution.resume_execution(value, &mut DummyExternals {})
        } else {
            assert!(value.is_none());       // TODO: turn into an error
            self.interrupted = true;
            execution.start_execution(&mut DummyExternals {})
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
                ExecOutcome::Interrupted(interrupt.index, interrupt.args.clone())
            }
            Err(wasmi::ResumableError::Trap(trap)) => {
                println!("oops, actual error!");
                // TODO: put in corrupted state?
                ExecOutcome::Errored(trap)
            }
        }
    }
}

pub enum ExecOutcome {
    Finished(Option<wasmi::RuntimeValue>),
    Interrupted(usize, Vec<wasmi::RuntimeValue>),
    Errored(wasmi::Trap),
}

struct DummyExternals {

}

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
