// Copyright(c) 2019 Pierre Krieger

#![feature(never_type)]
#![deny(intra_doc_link_resolution_failure)]

use byteorder::{ByteOrder as _, LittleEndian};
use parity_scale_codec::{DecodeAll, Encode as _};
use std::io::Write as _;

mod tcp_interface;
mod wasi;

fn main() {
    let module = kernel_core::module::Module::from_bytes(
        &include_bytes!("../../target/wasm32-wasi/release/ipfs.wasm")[..],
    );

    // TODO: signatures don't seem to be enforced
    // TODO: some of these have wrong signatures
    let mut system = kernel_core::system::System::new()
        .with_extrinsic(
            "wasi_unstable",
            "args_get",
            kernel_core::sig!((Pointer, Pointer)),
            Extrinsic::ArgsGet,
        )
        .with_extrinsic(
            "wasi_unstable",
            "args_sizes_get",
            kernel_core::sig!(() -> I32),
            Extrinsic::ArgsSizesGet,
        )
        .with_extrinsic(
            "wasi_unstable",
            "clock_time_get",
            kernel_core::sig!((I32, I64) -> I64),
            Extrinsic::ClockTimeGet,
        )
        .with_extrinsic(
            "wasi_unstable",
            "environ_get",
            kernel_core::sig!((Pointer, Pointer)),
            Extrinsic::EnvironGet,
        )
        .with_extrinsic(
            "wasi_unstable",
            "environ_sizes_get",
            kernel_core::sig!(() -> I32),
            Extrinsic::EnvironSizesGet,
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_prestat_get",
            kernel_core::sig!((I32, Pointer)),
            Extrinsic::FdPrestatGet,
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_prestat_dir_name",
            kernel_core::sig!((I32, Pointer, I32)),
            Extrinsic::FdPrestatDirName,
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_fdstat_get",
            kernel_core::sig!((I32, Pointer)),
            Extrinsic::FdFdstatGet,
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_write",
            kernel_core::sig!((I32, Pointer, I32) -> I32),
            Extrinsic::FdWrite,
        )
        .with_extrinsic(
            "wasi_unstable",
            "proc_exit",
            kernel_core::sig!((I32)),
            Extrinsic::ProcExit,
        )
        .with_interface_handler([
            // TCP
            0x10, 0x19, 0x16, 0x2a, 0x2b, 0x0c, 0x41, 0x36, 0x4a, 0x20, 0x01, 0x51, 0x47, 0x38,
            0x27, 0x08, 0x4a, 0x3c, 0x1e, 0x07, 0x18, 0x1c, 0x27, 0x11, 0x55, 0x15, 0x1d, 0x5f,
            0x22, 0x5b, 0x16, 0x20,
        ])
        .with_main_program(module)
        .build();

    let mut tcp = tcp_interface::TcpState::new();

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

    const ENV_VARS: &[u8] = b"RUST_BACKTRACE=1\0";

    loop {
        let result = futures::executor::block_on(async {
            loop {
                match system.run() {
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic: Extrinsic::ArgsGet,
                        params,
                    } => unimplemented!(),
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic: Extrinsic::ArgsSizesGet,
                        params,
                    } => {
                        assert_eq!(params.len(), 2);
                        let num_ptr = params[0].try_into::<i32>().unwrap() as u32;
                        let buf_size_ptr = params[1].try_into::<i32>().unwrap() as u32;
                        system.write_memory(pid, num_ptr, &[0, 0, 0, 0]).unwrap();
                        system.resolve_extrinsic_call(thread_id, Some(wasmi::RuntimeValue::I32(0)));
                        continue;
                    }
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic: Extrinsic::ClockTimeGet,
                        params,
                    } => unimplemented!(),
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic: Extrinsic::EnvironGet,
                        params,
                    } => {
                        assert_eq!(params.len(), 2);
                        let ptrs_ptr = params[0].try_into::<i32>().unwrap() as u32;
                        let buf_ptr = params[1].try_into::<i32>().unwrap() as u32;
                        let mut buf = [0; 4];
                        LittleEndian::write_u32(&mut buf, buf_ptr);
                        system.write_memory(pid, ptrs_ptr, &buf).unwrap();
                        system.write_memory(pid, buf_ptr, ENV_VARS).unwrap();
                        system.resolve_extrinsic_call(thread_id, Some(wasmi::RuntimeValue::I32(0)));
                        continue;
                    }
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic: Extrinsic::EnvironSizesGet,
                        params,
                    } => {
                        assert_eq!(params.len(), 2);
                        let num_ptr = params[0].try_into::<i32>().unwrap() as u32;
                        let buf_size_ptr = params[1].try_into::<i32>().unwrap() as u32;
                        let mut buf = [0; 4];
                        LittleEndian::write_u32(&mut buf, 1);
                        system.write_memory(pid, num_ptr, &buf).unwrap();
                        LittleEndian::write_u32(&mut buf, ENV_VARS.len() as u32);
                        system.write_memory(pid, buf_size_ptr, &buf).unwrap();
                        system.resolve_extrinsic_call(thread_id, Some(wasmi::RuntimeValue::I32(0)));
                        continue;
                    }
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic: Extrinsic::FdPrestatGet,
                        params,
                    } => {
                        assert_eq!(params.len(), 2);
                        let fd = params[0].try_into::<i32>().unwrap() as usize;
                        let ptr = params[1].try_into::<i32>().unwrap() as u32;
                        //system.write_memory(pid, ptr, &[0]).unwrap();
                        println!("prestat called with {:?}", fd);
                        // TODO: incorrect
                        system.resolve_extrinsic_call(thread_id, Some(wasmi::RuntimeValue::I32(8)));
                        continue;
                    }
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic: Extrinsic::FdPrestatDirName,
                        params,
                    } => unimplemented!(),
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic: Extrinsic::FdFdstatGet,
                        params,
                    } => unimplemented!(),
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic: Extrinsic::FdWrite,
                        params,
                    } => {
                        wasi::fd_write(&mut system, pid, thread_id, params);
                        continue;
                    }
                    kernel_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                        pid,
                        thread_id,
                        extrinsic: Extrinsic::ProcExit,
                        params,
                    } => unimplemented!(),
                    kernel_core::system::SystemRunOutcome::InterfaceMessage {
                        event_id,
                        interface,
                        message,
                    } => {
                        // TODO: we assume it's TCP
                        let message: tcp::ffi::TcpMessage =
                            DecodeAll::decode_all(&message).unwrap();
                        tcp.handle_message(event_id, message);
                        continue;
                    }
                    kernel_core::system::SystemRunOutcome::Idle => {}
                    other => break other,
                }

                let (msg_to_respond, response_bytes) = match tcp.next_event().await {
                    tcp_interface::TcpResponse::Open(msg_id, msg) => (msg_id, msg.encode()),
                    tcp_interface::TcpResponse::Read(msg_id, msg) => (msg_id, msg.encode()),
                    tcp_interface::TcpResponse::Write(msg_id, msg) => (msg_id, msg.encode()),
                };
                system.answer_event(msg_to_respond, &response_bytes);
            }
        });

        match result {
            kernel_core::system::SystemRunOutcome::ProgramFinished { pid, return_value } => {
                println!("Program finished {:?} => {:?}", pid, return_value)
            }
            kernel_core::system::SystemRunOutcome::ProgramCrashed { pid, error } => {
                println!("Program crashed {:?} => {:?}", pid, error);
            }
            _ => panic!(),
        }
    }
}
