// Copyright(c) 2019 Pierre Krieger

#![feature(never_type)]

fn main() {
    let mut core = core::core::Core::<()>::new();
    core.register_extrinsic([0; 32], "test", ());
    let module = core::module::Module::from_bytes(&include_bytes!("../../modules/preloaded/target/wasm32-unknown-unknown/release/preloaded.wasm")[..]);
    core.execute(&module).unwrap();

    loop {
        let result = futures::executor::block_on(core.run());
        println!("{:?}", result);
        match result {
            core::core::RunOutcome::ProgramWaitExtrinsic { pid, extrinsic } => {
                core.resolve_extrinsic_call(pid, Some(wasmi::RuntimeValue::I32(12)));
            }
            _ => break,
        }
    }
}
