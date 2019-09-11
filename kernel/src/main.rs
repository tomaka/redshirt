// Copyright(c) 2019 Pierre Krieger

#![feature(never_type)]

use std::sync::Arc;

fn main() {
    let module = kernel_core::module::Module::from_bytes(&include_bytes!("../../modules/preloaded/target/wasm32-wasi/release/preloaded.wasm")[..]);

    // TODO: signatures don't seem to be enforced
    let mut system = kernel_core::system::System::<Arc<dyn Fn(Vec<wasmi::RuntimeValue>) -> _>>::new()
        .with_extrinsic("wasi_unstable", "args_get", &kernel_core::sig!((Pointer, Pointer)), Arc::new(|params| {
            unimplemented!()
        }))
        .with_extrinsic("wasi_unstable", "args_sizes_get", &kernel_core::sig!(() -> I32), Arc::new(|params| {       // TODO: wrong output ype
            unimplemented!()
        }))
        .with_extrinsic("wasi_unstable", "clock_time_get", &kernel_core::sig!((I32, I64) -> I64), Arc::new(|params| {
            // TODO: do correctly
            Some(wasmi::RuntimeValue::I64(0x37))
        }))
        .with_extrinsic("wasi_unstable", "environ_get", &kernel_core::sig!((Pointer, Pointer)), Arc::new(|params| {
            unimplemented!()
        }))
        .with_extrinsic("wasi_unstable", "environ_sizes_get", &kernel_core::sig!(() -> I32), Arc::new(|params| {       // TODO: wrong output ype
            unimplemented!()
        }))
        .with_extrinsic("wasi_unstable", "fd_prestat_get", &kernel_core::sig!((I32, Pointer)), Arc::new(|params| {
            unimplemented!()
        }))
        .with_extrinsic("wasi_unstable", "fd_prestat_dir_name", &kernel_core::sig!((I32, Pointer, I32)), Arc::new(|params| {
            unimplemented!()
        }))
        .with_extrinsic("wasi_unstable", "fd_fdstat_get", &kernel_core::sig!((I32, Pointer)), Arc::new(|params| {
            unimplemented!()
        }))
        .with_extrinsic("wasi_unstable", "fd_write", &kernel_core::sig!((I32, Pointer, I32) -> I32), Arc::new(|params| {       // TODO: wrong params
            println!("{:?}", params);
            assert_eq!(params.len(), 3);
            assert!(params[0] == wasmi::RuntimeValue::I32(0) || params[0] == wasmi::RuntimeValue::I32(1));      // either stdout or stderr
            unimplemented!()
        }))
        .with_extrinsic("wasi_unstable", "proc_exit", &kernel_core::sig!((I32)), Arc::new(|params| {
            unimplemented!()
        }))
        .with_main_program(module)
        .build();

    loop {
        let result = futures::executor::block_on(async {
            loop {
                match system.run().await {
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic, params } => {
                        let ret = extrinsic(params);
                        system.resolve_extrinsic_call(pid, ret);
                    },
                    other => break other,
                }
            }
        });

        match result {
            kernel_core::system::SystemRunOutcome::ProgramFinished { pid, return_value } => {
                println!("Program finished {:?} => {:?}", pid, return_value)
            },
            kernel_core::system::SystemRunOutcome::ProgramCrashed { pid, error } => {
                println!("Program crashed {:?} => {:?}", pid, error);
            },
            _ => panic!()
        }
    }
}
