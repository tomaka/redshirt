// Copyright(c) 2019 Pierre Krieger

use crate::interface::InterfaceHash;
use crate::module::Module;
use crate::scheduler::{Core, CoreRunOutcome, Pid};
use alloc::borrow::Cow;
use smallvec::SmallVec;

pub struct System {
    core: Core<Extrinsic>,
}

pub struct SystemBuilder {
    core: Core<Extrinsic>,
    main_programs: SmallVec<[Module; 1]>,
}

#[derive(Debug)]
pub enum SystemRunOutcome {
    ProgramFinished {
        pid: Pid,
        return_value: Option<wasmi::RuntimeValue>,      // TODO: force to i32?
    },
    ProgramCrashed {
        pid: Pid,
        error: wasmi::Error,
    },
    // TODO: temporary; remove
    Nothing,
}

impl System {
    pub fn new() -> SystemBuilder {
        let mut core = Core::new()
            .with_extrinsic([0; 32], "register_interface", &wasmi::Signature::new(&[][..], Some(wasmi::ValueType::I32)), Extrinsic::RegisterInterface)
            .with_extrinsic([0; 32], "abort", &wasmi::Signature::new(&[][..], Some(wasmi::ValueType::I32)), Extrinsic::Abort)
            // TODO: remove randomess; that should be provided from outside of the core
            .with_extrinsic([0; 32], "get_random", &wasmi::Signature::new(&[][..], Some(wasmi::ValueType::I32)), Extrinsic::GetRandom)
            .build();

        SystemBuilder {
            core,
            main_programs: SmallVec::new(),
        }
    }

    pub async fn run(&mut self) -> SystemRunOutcome {
        loop {
            match self.core.run().await {
                CoreRunOutcome::ProgramFinished { pid, return_value } => {
                    return SystemRunOutcome::ProgramFinished { pid, return_value }
                },
                CoreRunOutcome::ProgramCrashed { pid, error } => {
                    return SystemRunOutcome::ProgramCrashed { pid, error }
                },
                CoreRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: &Extrinsic::GetRandom, params } => {
                    debug_assert!(params.is_empty());
                    self.core.resolve_extrinsic_call(pid, Some(wasmi::RuntimeValue::I32(4 /* randomly chosen value */)));     // TODO: 
                },
                CoreRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: &Extrinsic::RegisterInterface, params } => {
                    // TODO: implement
                    // self.core.set_interface_provider();
                    self.core.resolve_extrinsic_call(pid, None);
                },
                CoreRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: &Extrinsic::Abort, params } => {
                    debug_assert!(params.is_empty());
                    self.core.abort_process(pid).unwrap();
                },
                CoreRunOutcome::Nothing => {},
            }
        }
    }
}

impl SystemBuilder {
    pub fn with_extrinsic(mut self, interface: impl Into<InterfaceHash>, f_name: impl Into<Cow<'static, str>>, signature: &wasmi::Signature) -> Self {
        // TODO: implement
        self
    }

    /// Adds a program that the [`System`] must execute on startup. Can be called multiple times
    /// to add multiple programs.
    pub fn with_main_program(mut self, module: Module) -> Self {
        self.main_programs.push(module);
        self
    }

    /// Builds the [`System`].
    pub fn build(mut self) -> System {
        for program in self.main_programs.drain() {
            self.core.execute(&program).unwrap();       // TODO:
        }

        System {
            core: self.core,
        }
    }
}

#[derive(Debug)]
enum Extrinsic {
    RegisterInterface,
    Abort,
    GetRandom,
}
