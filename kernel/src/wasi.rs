// Copyright(c) 2019 Pierre Krieger

use byteorder::{ByteOrder as _, LittleEndian};
use kernel_core::scheduler::{Pid, ThreadId};
use kernel_core::system::{System, SystemBuilder};
use std::io::Write as _;

// TODO: lots of unwraps as `as` conversions in this module

/// Extrinsic related to WASI.
#[derive(Debug, Clone)]
pub struct WasiExtrinsic(WasiExtrinsicInner);

#[derive(Debug, Clone)]
enum WasiExtrinsicInner {
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
    SchedYield,
}

/// Adds to the `SystemBuilder` the extrinsics required by WASI.
pub fn register_extrinsics<T: From<WasiExtrinsic>>(system: SystemBuilder<T>) -> SystemBuilder<T> {
    // TODO: signatures don't seem to be enforced
    // TODO: some of these have wrong signatures
    system
        .with_extrinsic(
            "wasi_unstable",
            "args_get",
            kernel_core::sig!((Pointer, Pointer)),
            WasiExtrinsic(WasiExtrinsicInner::ArgsGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "args_sizes_get",
            kernel_core::sig!(() -> I32),
            WasiExtrinsic(WasiExtrinsicInner::ArgsSizesGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "clock_time_get",
            kernel_core::sig!((I32, I64) -> I64),
            WasiExtrinsic(WasiExtrinsicInner::ClockTimeGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "environ_get",
            kernel_core::sig!((Pointer, Pointer)),
            WasiExtrinsic(WasiExtrinsicInner::EnvironGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "environ_sizes_get",
            kernel_core::sig!(() -> I32),
            WasiExtrinsic(WasiExtrinsicInner::EnvironSizesGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_prestat_get",
            kernel_core::sig!((I32, Pointer)),
            WasiExtrinsic(WasiExtrinsicInner::FdPrestatGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_prestat_dir_name",
            kernel_core::sig!((I32, Pointer, I32)),
            WasiExtrinsic(WasiExtrinsicInner::FdPrestatDirName).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_fdstat_get",
            kernel_core::sig!((I32, Pointer)),
            WasiExtrinsic(WasiExtrinsicInner::FdFdstatGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_write",
            kernel_core::sig!((I32, Pointer, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::FdWrite).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "proc_exit",
            kernel_core::sig!((I32)),
            WasiExtrinsic(WasiExtrinsicInner::ProcExit).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "sched_yield",
            kernel_core::sig!(()),
            WasiExtrinsic(WasiExtrinsicInner::SchedYield).into(),
        )
}

pub fn handle_wasi(
    system: &mut System<impl Clone>,
    extrinsic: WasiExtrinsic,
    pid: Pid,
    thread_id: ThreadId,
    params: Vec<wasmi::RuntimeValue>,
) {
    const ENV_VARS: &[u8] = b"RUST_BACKTRACE=1\0";

    match extrinsic.0 {
        WasiExtrinsicInner::ArgsGet => unimplemented!(),
        WasiExtrinsicInner::ArgsSizesGet => {
            assert_eq!(params.len(), 2);
            let num_ptr = params[0].try_into::<i32>().unwrap() as u32;
            let buf_size_ptr = params[1].try_into::<i32>().unwrap() as u32;
            system.write_memory(pid, num_ptr, &[0, 0, 0, 0]).unwrap();
            system.resolve_extrinsic_call(thread_id, Some(wasmi::RuntimeValue::I32(0)));
        }
        WasiExtrinsicInner::ClockTimeGet => unimplemented!(),
        WasiExtrinsicInner::EnvironGet => {
            assert_eq!(params.len(), 2);
            let ptrs_ptr = params[0].try_into::<i32>().unwrap() as u32;
            let buf_ptr = params[1].try_into::<i32>().unwrap() as u32;
            let mut buf = [0; 4];
            LittleEndian::write_u32(&mut buf, buf_ptr);
            system.write_memory(pid, ptrs_ptr, &buf).unwrap();
            system.write_memory(pid, buf_ptr, ENV_VARS).unwrap();
            system.resolve_extrinsic_call(thread_id, Some(wasmi::RuntimeValue::I32(0)));
        }
        WasiExtrinsicInner::EnvironSizesGet => {
            assert_eq!(params.len(), 2);
            let num_ptr = params[0].try_into::<i32>().unwrap() as u32;
            let buf_size_ptr = params[1].try_into::<i32>().unwrap() as u32;
            let mut buf = [0; 4];
            LittleEndian::write_u32(&mut buf, 1);
            system.write_memory(pid, num_ptr, &buf).unwrap();
            LittleEndian::write_u32(&mut buf, ENV_VARS.len() as u32);
            system.write_memory(pid, buf_size_ptr, &buf).unwrap();
            system.resolve_extrinsic_call(thread_id, Some(wasmi::RuntimeValue::I32(0)));
        }
        WasiExtrinsicInner::FdPrestatGet => {
            assert_eq!(params.len(), 2);
            let fd = params[0].try_into::<i32>().unwrap() as usize;
            let ptr = params[1].try_into::<i32>().unwrap() as u32;
            //system.write_memory(pid, ptr, &[0]).unwrap();
            println!("prestat called with {:?}", fd);
            // TODO: incorrect
            system.resolve_extrinsic_call(thread_id, Some(wasmi::RuntimeValue::I32(8)));
        }
        WasiExtrinsicInner::FdPrestatDirName => unimplemented!(),
        WasiExtrinsicInner::FdFdstatGet => unimplemented!(),
        WasiExtrinsicInner::FdWrite => fd_write(system, pid, thread_id, params),
        WasiExtrinsicInner::ProcExit => unimplemented!(),
        WasiExtrinsicInner::SchedYield => {
            // TODO: guarantee the yield
            system.resolve_extrinsic_call(thread_id, Some(wasmi::RuntimeValue::I32(0)));
        },
    }
}

fn fd_write(
    system: &mut kernel_core::system::System<impl Clone>,
    pid: kernel_core::scheduler::Pid,
    thread_id: kernel_core::scheduler::ThreadId,
    params: Vec<wasmi::RuntimeValue>,
) {
    assert_eq!(params.len(), 4); // TODO: what to do when it's not the case?

    //assert!(params[0] == wasmi::RuntimeValue::I32(1) || params[0] == wasmi::RuntimeValue::I32(2));      // either stdout or stderr

    // Get a list of pointers and lengths to write.
    // Elements 0, 2, 4, 6, ... or that list are pointers, and elements 1, 3, 5, 7, ... are
    // lengths.
    let list_to_write = {
        let addr = params[1].try_into::<i32>().unwrap() as u32;
        let num = params[2].try_into::<i32>().unwrap() as u32;
        let list_buf = system.read_memory(pid, addr, 4 * num * 2).unwrap();
        let mut list_out = vec![0u32; (num * 2) as usize];
        LittleEndian::read_u32_into(&list_buf, &mut list_out);
        list_out
    };

    let mut total_written = 0;

    for ptr_and_len in list_to_write.windows(2) {
        let ptr = ptr_and_len[0] as u32;
        let len = ptr_and_len[1] as u32;

        let to_write = system.read_memory(pid, ptr, len).unwrap();
        std::io::stdout().write_all(&to_write).unwrap();
        total_written += to_write.len();
    }

    // Write to the fourth parameter the number of bytes written to the file descriptor.
    {
        let out_ptr = params[3].try_into::<i32>().unwrap() as u32;
        let mut buf = [0; 4];
        LittleEndian::write_u32(&mut buf, total_written as u32);
        system.write_memory(pid, out_ptr, &buf).unwrap();
    }

    system.resolve_extrinsic_call(thread_id, Some(wasmi::RuntimeValue::I32(0)));
}
