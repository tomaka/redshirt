// Copyright(c) 2019 Pierre Krieger

use crate::interface::InterfaceHash;
use crate::module::Module;
use crate::scheduler::{Core, CoreBuilder, CoreRunOutcome, Pid};
use crate::signature::{Signature, ValueType};
use alloc::borrow::Cow;
use core::iter;
use smallvec::SmallVec;

pub struct System<TExtEx> {
    core: Core<Extrinsic<TExtEx>>,
}

pub struct SystemBuilder<TExtEx> {
    core: CoreBuilder<Extrinsic<TExtEx>>,
    main_programs: SmallVec<[Module; 1]>,
}

#[derive(Debug)]
pub enum SystemRunOutcome<TExtEx> {
    ProgramFinished {
        pid: Pid,
        return_value: Option<wasmi::RuntimeValue>,      // TODO: force to i32?
    },
    ProgramCrashed {
        pid: Pid,
        error: wasmi::Error,
    },
    ProgramWaitExtrinsic {
        pid: Pid,
        extrinsic: TExtEx,
        params: Vec<wasmi::RuntimeValue>,
    },
    // TODO: temporary; remove
    Nothing,
}

impl<TExtEx: Clone> System<TExtEx> {
    pub fn new() -> SystemBuilder<TExtEx> {
        let mut core = Core::new()
            .with_extrinsic([0; 32], "register_interface", &Signature::new(iter::empty(), Some(ValueType::I32)), Extrinsic::RegisterInterface);

        SystemBuilder {
            core,
            main_programs: SmallVec::new(),
        }
    }

    /// After `ProgramWaitExtrinsic` has been returned, you have to call this method in order to
    /// inject back the result of the extrinsic call.
    pub fn resolve_extrinsic_call(&mut self, pid: Pid, return_value: Option<wasmi::RuntimeValue>) {
        // TODO: can the user badly misuse that API?
        self.core.resolve_extrinsic_call(pid, return_value);
    }

    pub async fn run(&mut self) -> SystemRunOutcome<TExtEx> {
        loop {
            match self.core.run().await {
                CoreRunOutcome::ProgramFinished { pid, return_value } => {
                    return SystemRunOutcome::ProgramFinished { pid, return_value }
                },
                CoreRunOutcome::ProgramCrashed { pid, error } => {
                    return SystemRunOutcome::ProgramCrashed { pid, error }
                },
                CoreRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: &Extrinsic::RegisterInterface, params } => {
                    // TODO: implement
                    // self.core.set_interface_provider();
                    self.core.resolve_extrinsic_call(pid, None);
                },
                CoreRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: &Extrinsic::External(ref external_token), ref params } => {
                    return SystemRunOutcome::ProgramWaitExtrinsic {
                        pid,
                        extrinsic: external_token.clone(),
                        params: params.clone(),
                    };
                },
                CoreRunOutcome::Nothing => {},
            }
        }
    }
}

impl<TExtEx> SystemBuilder<TExtEx> {
    pub fn with_extrinsic(mut self, interface: impl Into<InterfaceHash>, f_name: impl Into<Cow<'static, str>>, signature: &Signature, token: TExtEx) -> Self {
        self.core = self.core
            .with_extrinsic(interface, f_name, signature, Extrinsic::External(token));
        self
    }

    /// Adds a program that the [`System`] must execute on startup. Can be called multiple times
    /// to add multiple programs.
    pub fn with_main_program(mut self, module: Module) -> Self {
        self.main_programs.push(module);
        self
    }

    /// Builds the [`System`].
    pub fn build(mut self) -> System<TExtEx> {
        let mut core = self.core.build();

        for program in self.main_programs.drain() {
            core.execute(&program).unwrap();       // TODO:
        }

        System {
            core,
        }
    }
}

#[derive(Debug)]
enum Extrinsic<TExtEx> {
    RegisterInterface,
    External(TExtEx),
}
