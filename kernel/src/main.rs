// Copyright(c) 2019 Pierre Krieger

#![feature(never_type)]

use std::io::Write as _;

fn main() {
    let module = kernel_core::module::Module::from_bytes(&include_bytes!("../../modules/target/wasm32-wasi/release/preloaded.wasm")[..]);

    // TODO: signatures don't seem to be enforced
    // TODO: some of these have wrong signatures
    let mut system = kernel_core::system::System::new()
        .with_extrinsic("wasi_unstable", "args_get", &kernel_core::sig!((Pointer, Pointer)), Extrinsic::ArgsGet)
        .with_extrinsic("wasi_unstable", "args_sizes_get", &kernel_core::sig!(() -> I32), Extrinsic::ArgsSizesGet)
        .with_extrinsic("wasi_unstable", "clock_time_get", &kernel_core::sig!((I32, I64) -> I64), Extrinsic::ClockTimeGet)
        .with_extrinsic("wasi_unstable", "environ_get", &kernel_core::sig!((Pointer, Pointer)), Extrinsic::EnvironGet)
        .with_extrinsic("wasi_unstable", "environ_sizes_get", &kernel_core::sig!(() -> I32), Extrinsic::EnvironSizesGet)
        .with_extrinsic("wasi_unstable", "fd_prestat_get", &kernel_core::sig!((I32, Pointer)), Extrinsic::FdPrestatGet)
        .with_extrinsic("wasi_unstable", "fd_prestat_dir_name", &kernel_core::sig!((I32, Pointer, I32)), Extrinsic::FdPrestatDirName)
        .with_extrinsic("wasi_unstable", "fd_fdstat_get", &kernel_core::sig!((I32, Pointer)), Extrinsic::FdFdstatGet)
        .with_extrinsic("wasi_unstable", "fd_write", &kernel_core::sig!((I32, Pointer, I32) -> I32), Extrinsic::FdWrite)
        .with_extrinsic("wasi_unstable", "proc_exit", &kernel_core::sig!((I32)), Extrinsic::ProcExit)
        .with_main_program(module)
        .build();

    #[derive(Clone)]
    enum Extrinsic {
        ArgsGet,
        ArgsSizesGet,
        ClockTimeGet,
        EnvironGet,
        EnvironSizesGet,
        FdPrestatGet,
        FdPrestatDirName,
        FdFdstatGet,
        FdWrite,
        ProcExit,
    }

    loop {
        let result = futures::executor::block_on(async {
            loop {
                match system.run() {
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: Extrinsic::ArgsGet, params } => {
                        unimplemented!()
                    },
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: Extrinsic::ArgsSizesGet, params } => {
                        unimplemented!()
                    },
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: Extrinsic::ClockTimeGet, params } => {
                        unimplemented!()
                    },
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: Extrinsic::EnvironGet, params } => {
                        unimplemented!()
                    },
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: Extrinsic::EnvironSizesGet, params } => {
                        unimplemented!()
                    },
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: Extrinsic::FdPrestatGet, params } => {
                        unimplemented!()
                    },
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: Extrinsic::FdPrestatDirName, params } => {
                        unimplemented!()
                    },
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: Extrinsic::FdFdstatGet, params } => {
                        unimplemented!()
                    },
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: Extrinsic::FdWrite, params } => {
                        assert_eq!(params.len(), 4);
                        //assert!(params[0] == wasmi::RuntimeValue::I32(0) || params[0] == wasmi::RuntimeValue::I32(1));      // either stdout or stderr
                        let addr = params[1].try_into::<i32>().unwrap() as usize;
                        let mem = system.read_memory(pid, addr .. addr + 4);
                        let mem = ((mem[0] as u32) | ((mem[1] as u32) << 8) | ((mem[2] as u32) << 16) | ((mem[3] as u32) << 24)) as usize;
                        let buf_size = system.read_memory(pid, addr + 4 .. addr + 8);
                        let buf_size = ((buf_size[0] as u32) | ((buf_size[1] as u32) << 8) | ((buf_size[2] as u32) << 16) | ((buf_size[3] as u32) << 24)) as usize;
                        let buf = system.read_memory(pid, mem .. mem + buf_size);
                        std::io::stdout().write_all(&buf).unwrap();
                        system.resolve_extrinsic_call(pid, Some(wasmi::RuntimeValue::I32(buf.len() as i32)));
                    },
                    kernel_core::system::SystemRunOutcome::ProgramWaitExtrinsic { pid, extrinsic: Extrinsic::ProcExit, params } => {
                        unimplemented!()
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
