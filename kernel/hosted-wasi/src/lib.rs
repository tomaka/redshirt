// Copyright (C) 2019  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Handles calls that WASM code makes to the WASI API.
//!
//! The WASI API is implemented by translating WASI function calls into message emissions.
//!
//! # Usage
//!
//! - When building a system, register extrinsics using [`register_extrinsics`].
//! - Create a [`WasiStateMachine`].
//! - Whenever a program calls a WASI extrinsic, call [`WasiStateMachine::handle_extrinsic_call`].
//! - If [`HandleOut::EmitMessage`] is emitted, act as if a program had emitted the given message.
//! - When a message returned as a [`HandleOut::EmitMessage`] gets an answer, call
//!   [`WasiStateMachine::message_response`]. This function also returns [`HandleOut`] in case
//!   further calls are needed.
//!

#![no_std]

extern crate alloc;

use alloc::{string::String, string::ToString as _, vec, vec::Vec};
use byteorder::{ByteOrder as _, LittleEndian};
use core::convert::TryFrom as _;
use hashbrown::HashMap;
use redshirt_core::scheduler::{Pid, ThreadId};
use redshirt_core::system::{System, SystemBuilder};
use parity_scale_codec::{DecodeAll, Encode as _};

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
    RandomGet,
    SchedYield,
}

/// Identifier of a message emitted by the [`WasiStateMachine`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct WasiMessageId(u64);

/// Adds to the `SystemBuilder` the extrinsics required by WASI.
pub fn register_extrinsics<T: From<WasiExtrinsic> + Clone>(
    system: SystemBuilder<T>,
) -> SystemBuilder<T> {
    // TODO: remove Clone
    system
        .with_extrinsic(
            "wasi_unstable",
            "args_get",
            redshirt_core::sig!((I32, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::ArgsGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "args_sizes_get",
            redshirt_core::sig!((I32, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::ArgsSizesGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "clock_time_get",
            redshirt_core::sig!((I32, I64, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::ClockTimeGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "environ_get",
            redshirt_core::sig!((I32, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::EnvironGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "environ_sizes_get",
            redshirt_core::sig!((I32, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::EnvironSizesGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_prestat_get",
            redshirt_core::sig!((I32, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::FdPrestatGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_prestat_dir_name",
            redshirt_core::sig!((I32, I32, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::FdPrestatDirName).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_fdstat_get",
            redshirt_core::sig!((I32, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::FdFdstatGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "fd_write",
            redshirt_core::sig!((I32, I32, I32, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::FdWrite).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "proc_exit",
            redshirt_core::sig!((I32)),
            WasiExtrinsic(WasiExtrinsicInner::ProcExit).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "random_get",
            redshirt_core::sig!((I32, I32) -> I32),
            WasiExtrinsic(WasiExtrinsicInner::RandomGet).into(),
        )
        .with_extrinsic(
            "wasi_unstable",
            "sched_yield",
            redshirt_core::sig!(() -> I32),
            WasiExtrinsic(WasiExtrinsicInner::SchedYield).into(),
        )
}

/// State machine handling WASI extrinsics calls.
pub struct WasiStateMachine {
    /// Identifier of the next message to emit.
    next_id: WasiMessageId,
    /// All the interface messages that we emited and for which we're expecting a response.
    pending_messages: HashMap<WasiMessageId, CallInfo>,
}

enum CallInfo {
    Time(TimeCallInfo),
    Random(RandomCallInfo),
}

struct TimeCallInfo {
    pid: Pid,
    tid: ThreadId,
    /// Address of an 8 bytes buffer within the memory of `pid` where to write the result in
    /// little endian.
    out_ptr: u32,
}

struct RandomCallInfo {
    pid: Pid,
    tid: ThreadId,
    /// Pointer to the memory of `pid` of a buffer of length `remaining_len`.
    out_ptr: u32,
    /// Length of the buffer in `out_ptr` that must be filled with random data.
    remaining_len: u32,
}

#[must_use]
pub enum HandleOut {
    Ok,
    EmitMessage {
        id: Option<WasiMessageId>,
        interface: [u8; 32],
        message: Vec<u8>,
    },
}

impl WasiStateMachine {
    pub fn new() -> WasiStateMachine {
        WasiStateMachine {
            next_id: WasiMessageId(0),
            pending_messages: HashMap::new(),
        }
    }

    fn alloc_message_id(&mut self) -> WasiMessageId {
        let id = self.next_id;
        self.next_id.0 += 1;
        id
    }

    /// Call this when a process performs a WASI extrinsic call.
    pub fn handle_extrinsic_call(
        &mut self, // TODO: replace with `&self`
        system: &mut System<impl Clone>,
        extrinsic: WasiExtrinsic,
        pid: Pid,
        thread_id: ThreadId,
        params: Vec<redshirt_core::RuntimeValue>,
    ) -> HandleOut {
        const ENV_VARS: &[u8] = b"RUST_BACKTRACE=1\0";

        match extrinsic.0 {
            WasiExtrinsicInner::ArgsGet => unimplemented!(),
            WasiExtrinsicInner::ArgsSizesGet => {
                assert_eq!(params.len(), 2);
                let num_ptr = params[0].try_into::<i32>().unwrap() as u32;
                let buf_size_ptr = params[1].try_into::<i32>().unwrap() as u32;
                system.write_memory(pid, num_ptr, &[0, 0, 0, 0]).unwrap();
                system.resolve_extrinsic_call(thread_id, Some(redshirt_core::RuntimeValue::I32(0)));
                HandleOut::Ok
            }
            WasiExtrinsicInner::ClockTimeGet => {
                assert_eq!(params.len(), 3);
                // Note: precision is ignored
                let clock_ty = params[0].try_into::<i32>().unwrap();
                let out_ptr = params[2].try_into::<i32>().unwrap() as u32;
                let message = match clock_ty {
                    0 => {
                        // CLOCK_REALTIME
                        redshirt_time_interface::ffi::TimeMessage::GetSystem
                    }
                    1 => {
                        // CLOCK_MONOTONIC
                        redshirt_time_interface::ffi::TimeMessage::GetMonotonic
                    }
                    2 => {
                        // CLOCK_PROCESS_CPUTIME_ID
                        unimplemented!()
                    }
                    3 => {
                        // CLOCK_THREAD_CPUTIME_ID
                        unimplemented!()
                    }
                    _ => panic!(),
                };
                let msg_id = self.alloc_message_id();
                let _prev_val = self.pending_messages.insert(
                    msg_id,
                    CallInfo::Time(TimeCallInfo {
                        pid,
                        tid: thread_id,
                        out_ptr,
                    }),
                );
                debug_assert!(_prev_val.is_none());
                HandleOut::EmitMessage {
                    id: Some(msg_id),
                    interface: redshirt_time_interface::ffi::INTERFACE,
                    message: message.encode(),
                }
            }
            WasiExtrinsicInner::EnvironGet => {
                assert_eq!(params.len(), 2);
                let ptrs_ptr = params[0].try_into::<i32>().unwrap() as u32;
                let buf_ptr = params[1].try_into::<i32>().unwrap() as u32;
                let mut buf = [0; 4];
                LittleEndian::write_u32(&mut buf, buf_ptr);
                system.write_memory(pid, ptrs_ptr, &buf).unwrap();
                system.write_memory(pid, buf_ptr, ENV_VARS).unwrap();
                system.resolve_extrinsic_call(thread_id, Some(redshirt_core::RuntimeValue::I32(0)));
                HandleOut::Ok
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
                system.resolve_extrinsic_call(thread_id, Some(redshirt_core::RuntimeValue::I32(0)));
                HandleOut::Ok
            }
            WasiExtrinsicInner::FdPrestatGet => {
                assert_eq!(params.len(), 2);
                let fd = params[0].try_into::<i32>().unwrap() as usize;
                let ptr = params[1].try_into::<i32>().unwrap() as u32;
                //system.write_memory(pid, ptr, &[0]).unwrap();
                // TODO: incorrect
                system.resolve_extrinsic_call(thread_id, Some(redshirt_core::RuntimeValue::I32(8)));
                HandleOut::Ok
            }
            WasiExtrinsicInner::FdPrestatDirName => unimplemented!(),
            WasiExtrinsicInner::FdFdstatGet => unimplemented!(),
            WasiExtrinsicInner::FdWrite => fd_write(system, pid, thread_id, params),
            WasiExtrinsicInner::ProcExit => unimplemented!(),
            WasiExtrinsicInner::RandomGet => {
                assert_eq!(params.len(), 2);
                let buf = params[0].try_into::<i32>().unwrap() as u32;
                let len = params[1].try_into::<i32>().unwrap() as u32;

                let msg_id = self.alloc_message_id();
                let _prev_val = self.pending_messages.insert(
                    msg_id,
                    CallInfo::Random(RandomCallInfo {
                        pid,
                        tid: thread_id,
                        out_ptr: buf,
                        remaining_len: len,
                    }),
                );
                debug_assert!(_prev_val.is_none());

                let len_to_request = u16::try_from(len).unwrap_or(u16::max_value());
                let message = redshirt_random_interface::ffi::RandomMessage::Generate {
                    len: len_to_request,
                };
                HandleOut::EmitMessage {
                    id: Some(msg_id),
                    interface: redshirt_random_interface::ffi::INTERFACE,
                    message: message.encode(),
                }
            }
            WasiExtrinsicInner::SchedYield => {
                // TODO: guarantee the yield
                system.resolve_extrinsic_call(thread_id, Some(redshirt_core::RuntimeValue::I32(0)));
                HandleOut::Ok
            }
        }
    }

    // TODO: make `&self`
    pub fn message_response(
        &mut self,
        system: &mut System<impl Clone>,
        msg_id: WasiMessageId,
        response: Vec<u8>,
    ) -> HandleOut {
        match self.pending_messages.remove(&msg_id) {
            Some(CallInfo::Time(info)) => {
                let value: u128 = DecodeAll::decode_all(&response).unwrap();
                let to_write = u64::try_from(value).unwrap_or(u64::max_value()); // TODO: meh; return an error instead?
                let mut buf = [0; 8];
                LittleEndian::write_u64(&mut buf, to_write);
                system.write_memory(info.pid, info.out_ptr, &buf).unwrap();
                system.resolve_extrinsic_call(info.tid, Some(redshirt_core::RuntimeValue::I32(0)));
                HandleOut::Ok
            }
            Some(CallInfo::Random(mut info)) => {
                let value: redshirt_random_interface::ffi::GenerateResponse =
                    DecodeAll::decode_all(&response).unwrap();
                assert!(
                    u32::try_from(value.result.len()).unwrap_or(u32::max_value())
                        <= info.remaining_len
                );
                system
                    .write_memory(info.pid, info.out_ptr, &value.result)
                    .unwrap();
                info.remaining_len -= value.result.len() as u32; // TODO: as :-/

                if info.remaining_len == 0 {
                    system
                        .resolve_extrinsic_call(info.tid, Some(redshirt_core::RuntimeValue::I32(0)));
                    HandleOut::Ok
                } else {
                    let msg_id = self.alloc_message_id();
                    let len_to_request =
                        u16::try_from(info.remaining_len).unwrap_or(u16::max_value());

                    let _prev_val = self.pending_messages.insert(msg_id, CallInfo::Random(info));
                    debug_assert!(_prev_val.is_none());

                    let message = redshirt_random_interface::ffi::RandomMessage::Generate {
                        len: len_to_request,
                    };
                    HandleOut::EmitMessage {
                        id: Some(msg_id),
                        interface: redshirt_random_interface::ffi::INTERFACE,
                        message: message.encode(),
                    }
                }
            }
            None => panic!(), // TODO:
        }
    }
}

fn fd_write(
    system: &mut redshirt_core::system::System<impl Clone>,
    pid: redshirt_core::scheduler::Pid,
    thread_id: redshirt_core::scheduler::ThreadId,
    params: Vec<redshirt_core::RuntimeValue>,
) -> HandleOut {
    assert_eq!(params.len(), 4); // TODO: what to do when it's not the case?

    //assert!(params[0] == redshirt_core::RuntimeValue::I32(1) || params[0] == redshirt_core::RuntimeValue::I32(2));      // either stdout or stderr

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
    let mut to_write = Vec::new();

    for ptr_and_len in list_to_write.windows(2) {
        let ptr = ptr_and_len[0] as u32;
        let len = ptr_and_len[1] as u32;

        to_write.extend(system.read_memory(pid, ptr, len).unwrap());
        total_written += len as usize;
    }

    // Write to the fourth parameter the number of bytes written to the file descriptor.
    {
        let out_ptr = params[3].try_into::<i32>().unwrap() as u32;
        let mut buf = [0; 4];
        LittleEndian::write_u32(&mut buf, total_written as u32);
        system.write_memory(pid, out_ptr, &buf).unwrap();
    }

    system.resolve_extrinsic_call(thread_id, Some(redshirt_core::RuntimeValue::I32(0)));
    HandleOut::EmitMessage {
        id: None,
        interface: redshirt_stdout_interface::ffi::INTERFACE,
        message: redshirt_stdout_interface::ffi::StdoutMessage::Message(
            String::from_utf8_lossy(&to_write).to_string(),
        )
        .encode(), // TODO:  lossy?
    }
}
