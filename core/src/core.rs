// Copyright(c) 2019 Pierre Krieger

use crate::interface::InterfaceHash;
use crate::module::Module;

use alloc::collections::VecDeque;
use bimap::BiHashMap;
use hashbrown::{HashMap, HashSet};
use self::pid::{Pid, PidPool};
use std::fmt;

mod builder;
mod pid;

pub struct Core {
    next_pid: PidPool,
    loaded: HashMap<Pid, Program>,

    /// For each interface, which program is fulfilling it.
    interfaces: HashMap<InterfaceHash, Pid>,

    /// Holds a bijection between arbitrary values (the `usize` on the left side) that we pass
    /// to the WASM interpreter, and the function that corresponds to it.
    /// Whenever the interpreter wants to link to a function, we look for the `usize` corresponding
    /// to the requested function. When the interpreter wants to execute that function, it passes
    /// back that `usize` to us, and we can look which function it is.
    externals_indices: BiHashMap<usize, ([u32; 8], String)>,

    /// Queue of tasks to execute.
    scheduled: VecDeque<Scheduled>,
}

struct Program {
    module: wasmi::ModuleRef,       // TODO: ask serguey or someone whether that's a weak ref
    depends_on: Vec<Pid>,
    depended_on: HashSet<Pid>,
}

/// Tasks scheduled for execution.
struct Scheduled {
    pid: Pid,

    /// Context of a function being executed.
    execution: wasmi::FuncInvocation<'static>,

    /// If `Some`, then `execution` is in the middle of executing a function, and we need to
    /// provide back a value in order to continue. That value is an `Option<RuntimeValue>`.
    resume_value: Option<Option<wasmi::RuntimeValue>>,
}

impl Core {
    /// Initialies a new `Core`.
    pub fn new() -> Core {
        Core {
            next_pid: PidPool::new(),
            loaded: HashMap::with_capacity(128),
            interfaces: HashMap::with_capacity(32),
            externals_indices: BiHashMap::with_capacity(128),
            scheduled: VecDeque::with_capacity(32),
        }
    }

    pub fn has_interface(&self, interface: InterfaceHash) -> bool {
        self.interfaces.contains_key(&interface)
    }

    /// Returns a `Future` that runs the core.
    ///
    /// This returns a `Future` so that it is possible to interrupt the process.
    // TODO: make multithreaded
    pub async fn run(&mut self) {
        // TODO: wasi doesn't allow interrupting executions
        while let Some(mut scheduled) = self.scheduled.pop_front() {
            let result = if let Some(resume_value) = scheduled.resume_value {
                scheduled.execution.resume_execution(resume_value, &mut DummyExternals {})
            } else {
                scheduled.execution.start_execution(&mut DummyExternals {})
            };

            match result {
                Ok(Some(val)) => println!("status code: {:?}", val),
                Ok(None) => {},
                Err(wasmi::ResumableError::AlreadyStarted) => unreachable!(),
                Err(wasmi::ResumableError::NotResumable) => unreachable!(),
                Err(wasmi::ResumableError::Trap(trap)) => {
                    match trap.kind() {
                        wasmi::TrapKind::Host(host) => {
                            println!("{:?}", host);
                            // TODO: prototype hack
                            scheduled.resume_value = Some(Some(wasmi::RuntimeValue::I32(7)));
                            self.scheduled.push_back(scheduled);
                        },
                        _ => println!("oops, actual error!")
                    }
                }
            }
        }

        // TODO: sleep or something instead of terminating the future
    }

    /// Start executing the module passed as parameter.
    pub fn execute(&mut self, module: &Module) -> Result<Pid, ()> {
        let import_builder = EnvironmentDefinitionBuilder {};

        let not_started = wasmi::ModuleInstance::new(module.as_ref(), &import_builder).unwrap();      // TODO: don't unwrap
        let module = not_started.assert_no_start();     // TODO: true in practice, bad to do in theory

        let pid = self.next_pid.assign();
        match module.export_by_name("main") {
            Some(wasmi::ExternVal::Func(f)) => {
                let execution = wasmi::FuncInstance::invoke_resumable(&f, &[wasmi::RuntimeValue::I32(0), wasmi::RuntimeValue::I32(0)][..]).unwrap();
                self.scheduled.push_back(Scheduled {
                    pid,
                    execution,
                    resume_value: None,
                });
            },
            None => {},
            _ => panic!()       // TODO:
        }
        self.loaded.insert(pid, Program {
            module,
            depends_on: Vec::new(),
            depended_on: HashSet::new(),
        });
        Ok(pid)
    }
}

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

struct EnvironmentDefinitionBuilder {
}

impl wasmi::ImportResolver for EnvironmentDefinitionBuilder {
    fn resolve_func(&self, module_name: &str, field_name: &str, signature: &wasmi::Signature)
        -> Result<wasmi::FuncRef, wasmi::Error>
    {
        /*let key = (
            module_name.as_bytes().to_owned(),
            field_name.as_bytes().to_owned(),
        );
        let externval = self.map.get(&key).ok_or_else(|| {*/
            //Err(wasmi::Error::Instantiation(format!("Export {}:{} not found", module_name, field_name)))
        /*})?;
        let host_func_idx = match *externval {
            ExternVal::HostFunc(ref idx) => idx,
            _ => {
                return Err(wasmi::Error::Instantiation(format!(
                    "Export {}:{} is not a host func",
                    module_name, field_name
                )))
            }
        };
        Ok(FuncInstance::alloc_host(signature.clone(), host_func_idx.0))*/
        Ok(wasmi::FuncInstance::alloc_host(signature.clone(), 0))
    }

    fn resolve_global(&self, _module_name: &str, _field_name: &str, _global_type: &wasmi::GlobalDescriptor)
        -> Result<wasmi::GlobalRef, wasmi::Error>
    {
        Err(wasmi::Error::Instantiation("Importing globals is not supported yet".to_owned()))
    }

    fn resolve_memory(&self, module_name: &str, field_name: &str, _memory_type: &wasmi::MemoryDescriptor)
        -> Result<wasmi::MemoryRef, wasmi::Error>
    {
        /*let key = (
            module_name.as_bytes().to_owned(),
            field_name.as_bytes().to_owned(),
        );
        let externval = self.map.get(&key).ok_or_else(|| {*/
            Err(wasmi::Error::Instantiation(format!("Export {}:{} not found", module_name, field_name)))
        /*})?;
        let memory = match *externval {
            ExternVal::Memory(ref m) => m,
            _ => {
                return Err(wasmi::Error::Instantiation(format!(
                    "Export {}:{} is not a memory",
                    module_name, field_name
                )))
            }
        };
        Ok(memory.memref.clone())*/
    }

    fn resolve_table(&self, _module_name: &str, _field_name: &str, _table_type: &wasmi::TableDescriptor)
        -> Result<wasmi::TableRef, wasmi::Error>
    {
        Err(wasmi::Error::Instantiation("Importing tables is not supported yet".to_owned()))
    }
}

struct DummyExternals {

}

impl wasmi::Externals for DummyExternals {
    fn invoke_index(&mut self, _index: usize, _args: wasmi::RuntimeArgs)
        -> Result<Option<wasmi::RuntimeValue>, wasmi::Trap>
    {
        Err(wasmi::TrapKind::Host(Box::new(MyError { code: 5 })).into())
    }
}

#[derive(Debug)]
struct MyError {
    code: u32,
}

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MyError, code={}", self.code)
    }
}

impl wasmi::HostError for MyError { }
