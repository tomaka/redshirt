// Copyright(c) 2019 Pierre Krieger

#![feature(never_type)]

fn main() {
    let module = kernel_core::module::Module::from_bytes(&include_bytes!("../../modules/preloaded/target/wasm32-unknown-unknown/release/preloaded.wasm")[..]);
    let mut system = kernel_core::system::System::<()>::new()
        .with_extrinsic([0; 32], "get_random", &kernel_core::sig!(() -> I32), ())
        .with_main_program(module)
        .build();

    loop {
        let result = futures::executor::block_on(async {
            loop {
                match system.run().await {
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: (), params } => {
                        debug_assert!(params.is_empty());
                        system.resolve_extrinsic_call(pid, Some(wasmi::RuntimeValue::I32(rand::random())));
                    },
                    other => break other,
                }
            }
        });

        println!("{:?}", result);
    }
}
