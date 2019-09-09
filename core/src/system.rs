// Copyright(c) 2019 Pierre Krieger

use crate::core::{Core, RunOutcome};

pub struct System {
    core: Core<Extrinsic>,
}

impl System {
    pub fn new() -> Self {
        let mut core = Core::new()
            .with_extrinsic([0; 32], "get_random", Extrinsic::GetRandom)
            .build();

        System { core }
    }

    pub async fn run(&mut self) {
        match self.core.run().await {
            RunOutcome::ProgramFinished { pid, return_value } => {
            },
            RunOutcome::ProgramCrashed { pid, error } => {
            },
            RunOutcome::ProgramWaitExtrinsic { pid, extrinsic: &Extrinsic::GetRandom } => {
                self.core.resolve_extrinsic_call(pid, Some(wasmi::RuntimeValue::I32(4 /* randomly chosen value */)));     // TODO: 
            },
            RunOutcome::Nothing => {},
        }
    }
}

#[derive(Debug)]
enum Extrinsic {
    GetRandom,
}
