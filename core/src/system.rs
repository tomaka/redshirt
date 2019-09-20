// Copyright(c) 2019 Pierre Krieger

use crate::interface::InterfaceId;
use crate::module::Module;
use crate::scheduler::{Core, CoreBuilder, CoreProcess, CoreRunOutcome, Pid};
use crate::signature::{Signature, ValueType};
use alloc::borrow::Cow;
use core::{iter, ops::RangeBounds};
use parity_scale_codec::{Encode, Decode, DecodeAll};
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
        return_value: Option<wasmi::RuntimeValue>, // TODO: force to i32?
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
    Idle,
}

#[derive(Debug)]
enum Extrinsic<TExtEx> {
    RegisterInterface,
    External(TExtEx),
}

// TODO: we require Clone because of stupid borrowing issues; remove
impl<TExtEx: Clone> System<TExtEx> {
    pub fn new() -> SystemBuilder<TExtEx> {
        let mut core = Core::new().with_extrinsic(
            [0; 32],
            "register_interface",
            &Signature::new(iter::empty(), Some(ValueType::I32)),
            Extrinsic::RegisterInterface,
        );

        SystemBuilder {
            core,
            main_programs: SmallVec::new(),
        }
    }

    /// After `ProgramWaitExtrinsic` has been returned, you have to call this method in order to
    /// inject back the result of the extrinsic call.
    // TODO: don't expose wasmi::RuntimeValue
    pub fn resolve_extrinsic_call(&mut self, pid: Pid, return_value: Option<wasmi::RuntimeValue>) {
        // TODO: can the user badly misuse that API?
        self.core
            .process_by_id(pid)
            .unwrap()
            .resolve_extrinsic_call(return_value);
    }

    pub fn run(&mut self) -> SystemRunOutcome<TExtEx> {
        loop {
            match self.core.run() {
                CoreRunOutcome::ProgramFinished {
                    process,
                    return_value,
                } => {
                    return SystemRunOutcome::ProgramFinished {
                        pid: process.pid(),
                        return_value,
                    }
                }
                CoreRunOutcome::ProgramCrashed { pid, error } => {
                    return SystemRunOutcome::ProgramCrashed { pid, error }
                }
                CoreRunOutcome::ProgramWaitExtrinsic {
                    mut process,
                    extrinsic: &Extrinsic::RegisterInterface,
                    params,
                } => {
                    // TODO: implement
                    parse_register_interface(&mut process, params);
                    // self.core.set_interface_provider();
                    process.resolve_extrinsic_call(Some(wasmi::RuntimeValue::I32(5)));
                }
                CoreRunOutcome::ProgramWaitExtrinsic {
                    ref mut process,
                    extrinsic: &Extrinsic::External(ref external_token),
                    ref params,
                } => {
                    return SystemRunOutcome::ProgramWaitExtrinsic {
                        pid: process.pid(),
                        extrinsic: external_token.clone(),
                        params: params.clone(),
                    };
                }
                CoreRunOutcome::Idle => {}
            }
        }
    }

    /// Copies the given memory range of the given process into a `Vec<u8>`.
    ///
    /// Returns an error if the range is invalid.
    // TODO: should really return &mut [u8] I think
    pub fn read_memory(&mut self, pid: Pid, range: impl RangeBounds<usize>) -> Result<Vec<u8>, ()> {
        self.core.process_by_id(pid).ok_or(())?.read_memory(range)
    }
}

impl<TExtEx> SystemBuilder<TExtEx> {
    pub fn with_extrinsic(
        mut self,
        interface: impl Into<InterfaceId>,
        f_name: impl Into<Cow<'static, str>>,
        signature: &Signature,
        token: TExtEx,
    ) -> Self {
        self.core =
            self.core
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
            core.execute(&program).unwrap(); // TODO:
        }

        System { core }
    }
}

fn parse_register_interface<TExtEx>(process: &mut CoreProcess<Extrinsic<TExtEx>>, params: Vec<wasmi::RuntimeValue>) {
    assert_eq!(params.len(), 2);
    let mem = {
        let addr = params[0].try_into::<i32>().unwrap() as usize;
        let sz = params[1].try_into::<i32>().unwrap() as usize;
        process.read_memory(addr..addr + sz).unwrap()
    };

    let interface = syscalls::ffi::Interface::decode(&mut &mem[..]).unwrap();      // TODO: decode_all doesn't work; figure out why
    println!("{:?}", interface);
}
