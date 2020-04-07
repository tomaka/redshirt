// Copyright (C) 2019-2020  Pierre Krieger
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

//! Implementation of the [`Extrinsics`] trait that supports WASI.

// Reference for function signatures:
// https://github.com/WebAssembly/wasi-libc/blob/e1149ab0677317c6c981bcbb5e4c159e4d2b9669/libc-bottom-half/headers/public/wasi/api.h

use crate::extrinsics::{Extrinsics, ExtrinsicsAction, ExtrinsicsMemoryAccess, SupportedExtrinsic};
use crate::{sig, Encode as _, EncodedMessage, ThreadId, WasmValue};

use alloc::{
    borrow::Cow,
    string::{String, ToString as _},
    sync::Arc,
    vec,
    vec::{IntoIter, Vec},
};
use core::{cmp, convert::TryFrom as _, fmt, mem, slice};
use hashbrown::HashMap;
use spinning_top::Spinlock;

/// Implementation of the [`Extrinsics`] trait for WASI.
#[derive(Debug)]
pub struct WasiExtrinsics {
    /// Arguments passed to the program.
    args: Vec<Vec<u8>>,

    /// Environment variables passed to the program.
    env_vars: Vec<Vec<u8>>,

    /// List of open file descriptors.
    /// The integer representing the file descriptor is the index within that table. Since file
    /// descriptors must not change value over time, we instead replace them with `None` when
    /// closing.
    file_descriptors: Spinlock<Vec<Option<FileDescriptor>>>,

    /// Virtual file system accessible to the program.
    file_system: Arc<Inode>,
}

#[derive(Debug)]
enum FileDescriptor {
    /// Valid file descriptor but that points to nothing.
    Empty,
    LogOut {
        /// We buffer data and emit a log message only on line splits.
        buffer: Vec<u8>,
        level: redshirt_log_interface::Level,
    },
    FilesystemEntry {
        inode: Arc<Inode>,
        /// Position of the cursor within the file. Always 0 for directories.
        file_cursor_pos: u64,
    },
}

#[derive(Debug)]
enum Inode {
    Directory {
        entries: Spinlock<HashMap<String, Arc<Inode>, fnv::FnvBuildHasher>>,
    },
    File {
        content: Vec<u8>,
    },
}

impl Default for WasiExtrinsics {
    fn default() -> WasiExtrinsics {
        let fs_root = Arc::new(Inode::Directory {
            entries: Spinlock::new({
                let mut hashmap = HashMap::default();
                /*// TODO: hack to toy with DOOM
                hashmap.insert(
                    "doom1.wad".to_string(),
                    Arc::new(Inode::File {
                        content: include_bytes!("../../../../DOOM/doom1.wad").to_vec(),
                    }),
                );*/
                hashmap
            }),
        });

        WasiExtrinsics {
            args: vec![b"foo".to_vec()], // TODO: "foo" is a dummy program name
            env_vars: vec![b"HOME=/home".to_vec()], // TODO: dummy
            file_descriptors: Spinlock::new(vec![
                // stdin
                Some(FileDescriptor::Empty),
                // stdout
                Some(FileDescriptor::LogOut {
                    level: redshirt_log_interface::Level::Info,
                    buffer: Vec::new(),
                }),
                // stderr
                Some(FileDescriptor::LogOut {
                    level: redshirt_log_interface::Level::Error,
                    buffer: Vec::new(),
                }),
                // pre-opened access to filesystem
                Some(FileDescriptor::FilesystemEntry {
                    inode: fs_root.clone(),
                    file_cursor_pos: 0,
                }),
            ]),
            file_system: fs_root.clone(),
        }
    }
}

/// Identifier of a WASI extrinsic.
#[derive(Debug, Clone)]
pub struct ExtrinsicId(ExtrinsicIdInner);

#[derive(Debug, Clone)]
enum ExtrinsicIdInner {
    ArgsGet,
    ArgsSizesGet,
    ClockTimeGet,
    EnvironGet,
    EnvironSizesGet,
    FdClose,
    FdFdstatGet,
    FdFdstatSetFlags,
    FdFilestatGet,
    FdPrestatDirName,
    FdPrestatGet,
    FdRead,
    FdSeek,
    FdTell,
    FdWrite,
    PathCreateDirectory,
    PathFilestatGet,
    PathOpen,
    PollOneOff,
    ProcExit,
    RandomGet,
    SchedYield,
}

/// Context for a call to a WASI external function.
pub struct Context(ContextInner);

enum ContextInner {
    WaitClockVal { out_ptr: u32 },
    WaitRandom { out_ptr: u32, remaining_len: u32 },
    TryFlushLogOut(usize),
    Resume(Option<WasmValue>),
    Finished,
}

impl Extrinsics for WasiExtrinsics {
    type ExtrinsicId = ExtrinsicId;
    type Context = Context;
    type Iterator = IntoIter<SupportedExtrinsic<Self::ExtrinsicId>>;

    fn supported_extrinsics() -> Self::Iterator {
        vec![
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::ArgsGet),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("args_get"),
                signature: sig!((I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::ArgsSizesGet),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("args_sizes_get"),
                signature: sig!((I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::ClockTimeGet),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("clock_time_get"),
                signature: sig!((I32, I64, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::EnvironGet),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("environ_get"),
                signature: sig!((I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::EnvironSizesGet),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("environ_sizes_get"),
                signature: sig!((I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::FdClose),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_close"),
                signature: sig!((I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::FdFdstatGet),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_fdstat_get"),
                signature: sig!((I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::FdFdstatSetFlags),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_fdstat_set_flags"),
                signature: sig!((I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::FdFilestatGet),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_filestat_get"),
                signature: sig!((I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::FdPrestatDirName),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_prestat_dir_name"),
                signature: sig!((I32, I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::FdPrestatGet),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_prestat_get"),
                signature: sig!((I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::FdRead),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_read"),
                signature: sig!((I32, I32, I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::FdSeek),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_seek"),
                signature: sig!((I32, I64, I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::FdTell),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_tell"),
                signature: sig!((I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::FdWrite),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_write"),
                signature: sig!((I32, I32, I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::PathCreateDirectory),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("path_create_directory"),
                signature: sig!((I32, I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::PathFilestatGet),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("path_filestat_get"),
                signature: sig!((I32, I32, I32, I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::PathOpen),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("path_open"),
                signature: sig!((I32, I32, I32, I32, I32, I64, I64, I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::PollOneOff),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("poll_oneoff"),
                signature: sig!((I32, I32, I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::ProcExit),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("proc_exit"),
                signature: sig!((I32)),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::RandomGet),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("random_get"),
                signature: sig!((I32, I32) -> I32),
            },
            SupportedExtrinsic {
                id: ExtrinsicId(ExtrinsicIdInner::SchedYield),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("sched_yield"),
                signature: sig!(() -> I32),
            },
        ]
        .into_iter()
    }

    fn new_context(
        &self,
        _: ThreadId,
        id: &Self::ExtrinsicId,
        params: impl ExactSizeIterator<Item = WasmValue>,
        mem_access: &mut impl ExtrinsicsMemoryAccess,
    ) -> (Self::Context, ExtrinsicsAction) {
        // All these function calls have the same return type. They return an error if there is
        // something fundamentally wrong in the system call (for example: a pointer to
        // out-of-bounds memory) and we have to make the program crash.
        let result = match id.0 {
            ExtrinsicIdInner::ArgsGet => args_get(self, params, mem_access),
            ExtrinsicIdInner::ArgsSizesGet => args_sizes_get(self, params, mem_access),
            ExtrinsicIdInner::ClockTimeGet => clock_time_get(self, params, mem_access),
            ExtrinsicIdInner::EnvironGet => environ_get(self, params, mem_access),
            ExtrinsicIdInner::EnvironSizesGet => environ_sizes_get(self, params, mem_access),
            ExtrinsicIdInner::FdClose => fd_close(self, params, mem_access),
            ExtrinsicIdInner::FdFdstatGet => fd_fdstat_get(self, params, mem_access),
            ExtrinsicIdInner::FdFdstatSetFlags => unimplemented!(),
            ExtrinsicIdInner::FdFilestatGet => fd_filestat_get(self, params, mem_access),
            ExtrinsicIdInner::FdPrestatDirName => fd_prestat_dir_name(self, params, mem_access),
            ExtrinsicIdInner::FdPrestatGet => fd_prestat_get(self, params, mem_access),
            ExtrinsicIdInner::FdRead => fd_read(self, params, mem_access),
            ExtrinsicIdInner::FdSeek => fd_seek(self, params, mem_access),
            ExtrinsicIdInner::FdTell => unimplemented!(),
            ExtrinsicIdInner::FdWrite => fd_write(self, params, mem_access),
            ExtrinsicIdInner::PathCreateDirectory => unimplemented!(),
            ExtrinsicIdInner::PathFilestatGet => path_filestat_get(self, params, mem_access),
            ExtrinsicIdInner::PathOpen => path_open(self, params, mem_access),
            ExtrinsicIdInner::PollOneOff => poll_oneoff(self, params, mem_access),
            ExtrinsicIdInner::ProcExit => proc_exit(self, params, mem_access),
            ExtrinsicIdInner::RandomGet => random_get(self, params, mem_access),
            ExtrinsicIdInner::SchedYield => sched_yield(self, params, mem_access),
        };

        match result {
            Ok((context, action)) => (Context(context), action),
            Err(WasiCallErr) => (
                Context(ContextInner::Finished),
                ExtrinsicsAction::ProgramCrash,
            ),
        }
    }

    fn inject_message_response(
        &self,
        ctxt: &mut Self::Context,
        response: Option<EncodedMessage>,
        mem_access: &mut impl ExtrinsicsMemoryAccess,
    ) -> ExtrinsicsAction {
        match ctxt.0 {
            ContextInner::WaitClockVal { out_ptr } => {
                let response = response.unwrap();
                let value: u128 = match response.decode() {
                    Ok(v) => v,
                    Err(_) => return ExtrinsicsAction::ProgramCrash,
                };

                let converted_value: wasi::Timestamp =
                    wasi::Timestamp::try_from(value % u128::from(wasi::Timestamp::max_value()))
                        .unwrap();
                mem_access
                    .write_memory(out_ptr, &converted_value.to_le_bytes())
                    .unwrap(); // TODO: don't unwrap

                ctxt.0 = ContextInner::Finished;
                ExtrinsicsAction::Resume(Some(WasmValue::I32(0)))
            }
            ContextInner::WaitRandom {
                mut out_ptr,
                mut remaining_len,
            } => {
                let response = response.unwrap();
                let value: redshirt_random_interface::ffi::GenerateResponse =
                    match response.decode() {
                        Ok(v) => v,
                        Err(_) => return ExtrinsicsAction::ProgramCrash,
                    };

                assert!(
                    u32::try_from(value.result.len()).unwrap_or(u32::max_value())
                        <= u32::from(remaining_len)
                );
                mem_access.write_memory(out_ptr, &value.result).unwrap(); // TODO: don't unwrap

                assert_ne!(value.result.len(), 0); // TODO: don't unwrap
                out_ptr += u32::try_from(value.result.len()).unwrap();
                remaining_len -= u32::try_from(value.result.len()).unwrap();

                if remaining_len == 0 {
                    ctxt.0 = ContextInner::Finished;
                    ExtrinsicsAction::Resume(Some(WasmValue::I32(0)))
                } else {
                    let len_to_request = u16::try_from(remaining_len).unwrap_or(u16::max_value());
                    debug_assert!(u32::from(len_to_request) <= remaining_len);

                    ctxt.0 = ContextInner::WaitRandom {
                        out_ptr,
                        remaining_len,
                    };

                    ExtrinsicsAction::EmitMessage {
                        interface: redshirt_random_interface::ffi::INTERFACE,
                        message: redshirt_random_interface::ffi::RandomMessage::Generate {
                            len: len_to_request,
                        }
                        .encode(),
                        response_expected: true,
                    }
                }
            }
            ContextInner::TryFlushLogOut(fd) => {
                let mut file_descriptors_lock = self.file_descriptors.lock();
                let file_descriptor = {
                    match file_descriptors_lock.get_mut(fd).and_then(|v| v.as_mut()) {
                        Some(fd) => fd,
                        None => {
                            ctxt.0 = ContextInner::Finished;
                            return ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
                        }
                    }
                };

                if let FileDescriptor::LogOut { level, buffer } = file_descriptor {
                    if let Some(split_pos) = buffer.iter().position(|c| *c == b'\n') {
                        let mut encoded_message = Vec::new();
                        encoded_message.push(u8::from(*level));
                        encoded_message.extend(buffer.drain(..split_pos));
                        buffer.remove(0);
        
                        let action = ExtrinsicsAction::EmitMessage {
                            interface: redshirt_log_interface::ffi::INTERFACE,
                            message: EncodedMessage(encoded_message),
                            response_expected: false,
                        };
        
                        ctxt.0 = ContextInner::TryFlushLogOut(fd);
                        action
        
                    } else {
                        ctxt.0 = ContextInner::Finished;
                        ExtrinsicsAction::Resume(Some(WasmValue::I32(0)))
                    }
                } else {
                    ctxt.0 = ContextInner::Finished;
                    ExtrinsicsAction::Resume(Some(WasmValue::I32(0)))
                }
            }
            ContextInner::Resume(value) => {
                ctxt.0 = ContextInner::Finished;
                ExtrinsicsAction::Resume(value)
            }
            ContextInner::Finished => unreachable!(),
        }
    }
}

// Implementations of WASI function calls below.
//
// # About unwrapping and panics
//
// It is allowed and encouraged to panic in case of an anomaly in the number or the types of
// arguments. This is because function signatures have normally been verified before the call is
// made.
//
// Any other error condition, including for example converting `i32` parameters to `u32`, should
// be handled by not panicking.

/// Dummy error type that "absorbs" all possible error types.
struct WasiCallErr;
impl<T: fmt::Debug> From<T> for WasiCallErr {
    fn from(_: T) -> Self {
        WasiCallErr
    }
}

fn args_get(
    state: &WasiExtrinsics,
    params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    args_or_env_get(&state.args, params, mem_access)
}

fn args_sizes_get(
    state: &WasiExtrinsics,
    params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    args_or_env_sizes_get(&state.args, params, mem_access)
}

fn clock_time_get(
    _: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let clock_id = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    let _precision = params.next().unwrap().into_i64().unwrap();

    let time_out = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    match clock_id {
        wasi::CLOCKID_REALTIME => {
            let action = ExtrinsicsAction::EmitMessage {
                interface: redshirt_system_time_interface::ffi::INTERFACE,
                message: redshirt_system_time_interface::ffi::TimeMessage::GetSystem.encode(),
                response_expected: true,
            };

            let context = ContextInner::WaitClockVal { out_ptr: time_out };
            Ok((context, action))
        }
        wasi::CLOCKID_MONOTONIC => {
            let action = ExtrinsicsAction::EmitMessage {
                interface: redshirt_time_interface::ffi::INTERFACE,
                message: redshirt_time_interface::ffi::TimeMessage::GetMonotonic.encode(),
                response_expected: true,
            };

            let context = ContextInner::WaitClockVal { out_ptr: time_out };
            Ok((context, action))
        }
        wasi::CLOCKID_PROCESS_CPUTIME_ID => unimplemented!(), // TODO:
        wasi::CLOCKID_THREAD_CPUTIME_ID => unimplemented!(),  // TODO:
        _ => return Err(WasiCallErr),
    }
}

fn environ_get(
    state: &WasiExtrinsics,
    params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    args_or_env_get(&state.env_vars, params, mem_access)
}

fn environ_sizes_get(
    state: &WasiExtrinsics,
    params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    args_or_env_sizes_get(&state.env_vars, params, mem_access)
}

fn fd_close(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let mut file_descriptors_lock = state.file_descriptors.lock();

    let fd = usize::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    // Check validity of the file descriptor.
    if file_descriptors_lock
        .get(fd)
        .map(|f| f.is_none())
        .unwrap_or(true)
    {
        let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
        let action = ExtrinsicsAction::Resume(ret);
        return Ok((ContextInner::Finished, action));
    }

    file_descriptors_lock[fd] = None;

    // Clean up the tail of `file_descriptors_lock`.
    while file_descriptors_lock
        .last()
        .map(|f| f.is_none())
        .unwrap_or(false)
    {
        file_descriptors_lock.pop();
    }
    file_descriptors_lock.shrink_to_fit();

    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

fn fd_fdstat_get(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to write to.
    let file_descriptor = {
        let fd = usize::try_from(params.next().unwrap().into_i32().unwrap())?;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return Ok((ContextInner::Finished, action));
            }
        }
    };

    // TODO: we mimic what wasmtime does, but documentation about these rights is pretty sparse
    let dirs_rights = wasi::RIGHTS_FD_FDSTAT_SET_FLAGS
        | wasi::RIGHTS_FD_SYNC
        | wasi::RIGHTS_FD_ADVISE
        | wasi::RIGHTS_PATH_CREATE_DIRECTORY
        | wasi::RIGHTS_PATH_CREATE_FILE
        | wasi::RIGHTS_PATH_LINK_SOURCE
        | wasi::RIGHTS_PATH_LINK_TARGET
        | wasi::RIGHTS_PATH_OPEN
        | wasi::RIGHTS_FD_READDIR
        | wasi::RIGHTS_PATH_READLINK
        | wasi::RIGHTS_PATH_RENAME_SOURCE
        | wasi::RIGHTS_PATH_RENAME_TARGET
        | wasi::RIGHTS_PATH_FILESTAT_GET
        | wasi::RIGHTS_PATH_FILESTAT_SET_SIZE
        | wasi::RIGHTS_PATH_FILESTAT_SET_TIMES
        | wasi::RIGHTS_FD_FILESTAT_GET
        | wasi::RIGHTS_FD_FILESTAT_SET_SIZE
        | wasi::RIGHTS_FD_FILESTAT_SET_TIMES
        | wasi::RIGHTS_PATH_SYMLINK
        | wasi::RIGHTS_PATH_REMOVE_DIRECTORY
        | wasi::RIGHTS_PATH_UNLINK_FILE
        | wasi::RIGHTS_POLL_FD_READWRITE;
    let files_rights = wasi::RIGHTS_FD_DATASYNC
        | wasi::RIGHTS_FD_READ
        | wasi::RIGHTS_FD_SEEK
        | wasi::RIGHTS_FD_FDSTAT_SET_FLAGS
        | wasi::RIGHTS_FD_SYNC
        | wasi::RIGHTS_FD_TELL
        | wasi::RIGHTS_FD_WRITE
        | wasi::RIGHTS_FD_ADVISE
        | wasi::RIGHTS_FD_ALLOCATE
        | wasi::RIGHTS_FD_FILESTAT_GET
        | wasi::RIGHTS_FD_FILESTAT_SET_SIZE
        | wasi::RIGHTS_FD_FILESTAT_SET_TIMES
        | wasi::RIGHTS_POLL_FD_READWRITE;

    let stat = match file_descriptor {
        FileDescriptor::Empty => wasi::Fdstat {
            fs_filetype: wasi::FILETYPE_CHARACTER_DEVICE,
            fs_flags: 0,
            fs_rights_base: 0,
            fs_rights_inheriting: 0,
        },
        FileDescriptor::LogOut { .. } => wasi::Fdstat {
            fs_filetype: wasi::FILETYPE_CHARACTER_DEVICE,
            fs_flags: wasi::FDFLAGS_APPEND,
            fs_rights_base: 0x820004a, // TODO: that's what wasmtime returns, don't know what it means
            fs_rights_inheriting: 0x820004a, // TODO: that's what wasmtime returns, don't know what it means
        },
        FileDescriptor::FilesystemEntry { inode, .. } => match **inode {
            Inode::Directory { .. } => wasi::Fdstat {
                fs_filetype: wasi::FILETYPE_DIRECTORY,
                fs_flags: 0,
                fs_rights_base: dirs_rights,
                fs_rights_inheriting: files_rights | dirs_rights,
            },
            Inode::File { .. } => wasi::Fdstat {
                fs_filetype: wasi::FILETYPE_REGULAR_FILE,
                fs_flags: 0,
                fs_rights_base: files_rights,
                fs_rights_inheriting: files_rights,
            },
        },
    };

    let stat_out_buf = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    // Note: this is a bit of dark magic, but it is the only solution at the moment.
    // Can be tested with the following snippet:
    // ```c
    // #include <stdio.h>
    // #include <wasi/api.h>
    // int main() {
    //     __wasi_fdstat_t* ptr = (__wasi_fdstat_t*)0x1000;
    //     printf("%p %p %p %p %p %d\n", ptr, &ptr->fs_filetype, &ptr->fs_flags, &ptr->fs_rights_base, &ptr->fs_rights_inheriting, sizeof(__wasi_fdstat_t));
    //     return 0;
    // }
    // ```
    // Which prints `0x1000 0x1000 0x1002 0x1008 0x1010 24`
    mem_access.write_memory(stat_out_buf, &[0; 24])?;
    mem_access.write_memory(stat_out_buf, &[stat.fs_filetype])?;
    mem_access.write_memory(stat_out_buf.checked_add(2)?, &stat.fs_flags.to_le_bytes())?;
    mem_access.write_memory(
        stat_out_buf.checked_add(8)?,
        &stat.fs_rights_base.to_le_bytes(),
    )?;
    mem_access.write_memory(
        stat_out_buf.checked_add(16)?,
        &stat.fs_rights_inheriting.to_le_bytes(),
    )?;

    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

fn fd_filestat_get(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to write to.
    let file_descriptor = {
        let fd = usize::try_from(params.next().unwrap().into_i32().unwrap())?;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return Ok((ContextInner::Finished, action));
            }
        }
    };

    let _stat_out_buf = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    unimplemented!();
}

fn fd_prestat_dir_name(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to write to.
    let file_descriptor = {
        let fd = usize::try_from(params.next().unwrap().into_i32().unwrap())?;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return Ok((ContextInner::Finished, action));
            }
        }
    };

    let name = match file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut { .. } => {
            // TODO: is that the correct return type?
            let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
            let action = ExtrinsicsAction::Resume(ret);
            return Ok((ContextInner::Finished, action));
        }
        // TODO: correct name; note that no null terminator is needed
        // note that apparently any value other than an empty string will fail to match relative paths? it's weird
        // cc https://github.com/CraneStation/wasi-libc/blob/9efc2f428358564fe64c374d762d0bfce1d92507/libc-bottom-half/libpreopen/libpreopen.c#L470
        FileDescriptor::FilesystemEntry { .. } => b"",
    };

    let path_out = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    let path_out_len =
        usize::try_from(params.next().unwrap().into_i32().unwrap()).unwrap_or(usize::max_value());
    assert!(params.next().is_none());

    // TODO: is it correct to truncate if the buffer is too small?
    let to_write = cmp::min(path_out_len, name.len());
    mem_access.write_memory(path_out, &name[..to_write])?;

    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

fn fd_prestat_get(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to write to.
    let file_descriptor = {
        let fd = usize::try_from(params.next().unwrap().into_i32().unwrap())?;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return Ok((ContextInner::Finished, action));
            }
        }
    };

    let pr_name_len: u32 = match file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut { .. } => {
            let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_NOTSUP)));
            let action = ExtrinsicsAction::Resume(ret);
            return Ok((ContextInner::Finished, action));
        }
        FileDescriptor::FilesystemEntry { inode, .. } => match **inode {
            // TODO: we don't know for sure that it's been pre-open
            Inode::Directory { .. } => 0, // TODO: must match the length of the return value of `fd_prestat_dir_name`
            Inode::File { .. } => {
                // TODO: is that the correct return type?
                let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_NOTSUP)));
                let action = ExtrinsicsAction::Resume(ret);
                return Ok((ContextInner::Finished, action));
            }
        },
    };

    let prestat_out_buf = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    // Note: this is a bit of dark magic, but it is the only solution at the moment.
    // Can be tested with the following snippet:
    // ```c
    // #include <stdio.h>
    // #include <wasi/api.h>
    // int main() {
    //     __wasi_prestat_t* ptr = (__wasi_prestat_t*)0x1000;
    //     printf("%p %p %p %d\n", ptr, &ptr->tag, &ptr->u.dir, sizeof(__wasi_prestat_t));
    //     return 0;
    // }
    // ```
    // Which prints `0x1000 0x1000 0x1004 8`
    mem_access.write_memory(prestat_out_buf, &[0; 8])?;
    mem_access.write_memory(prestat_out_buf, &[wasi::PREOPENTYPE_DIR])?;
    mem_access.write_memory(prestat_out_buf.checked_add(4)?, &pr_name_len.to_le_bytes())?;

    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

fn fd_read(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let mut file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to read from.
    let mut file_descriptor = {
        let fd = usize::try_from(params.next().unwrap().into_i32().unwrap())?;
        match file_descriptors_lock.get_mut(fd).and_then(|v| v.as_mut()) {
            Some(fd) => fd,
            None => {
                let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return Ok((ContextInner::Finished, action));
            }
        }
    };

    // Get a list of pointers and lengths to read to.
    // Elements 0, 2, 4, 6, ... in that list are pointers, and elements 1, 3, 5, 7, ... are
    // lengths.
    let out_buffers_list = {
        let addr = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
        let num = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
        let list_buf = mem_access.read_memory(addr..addr + 4 * num * 2)?;
        // TODO: don't panic if allocation size is too large
        let mut list_out = Vec::with_capacity(usize::try_from(num)?);
        for elem in list_buf.chunks(4) {
            list_out.push(u32::from_le_bytes(<[u8; 4]>::try_from(elem).unwrap()));
        }
        list_out
    };

    let total_read: u32 = match &mut file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut { .. } => 0,
        FileDescriptor::FilesystemEntry {
            inode,
            file_cursor_pos,
        } => {
            match &**inode {
                Inode::Directory { .. } => {
                    // TODO: is that the correct error?
                    let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                    let action = ExtrinsicsAction::Resume(ret);
                    return Ok((ContextInner::Finished, action));
                }
                Inode::File { content, .. } => {
                    let mut total_read = 0;
                    for buffer in out_buffers_list.chunks(2) {
                        let buffer_ptr = buffer[0];
                        let buffer_len = usize::try_from(buffer[1])?;
                        // The cursor position cannot go past `content.len()`, and thus always
                        // fits in a `usize`.
                        let file_cursor_pos_usize = usize::try_from(*file_cursor_pos).unwrap();
                        debug_assert!(file_cursor_pos_usize <= content.len());
                        let to_copy = cmp::min(content.len() - file_cursor_pos_usize, buffer_len);
                        if to_copy == 0 {
                            break;
                        }
                        mem_access.write_memory(
                            buffer_ptr,
                            &content[file_cursor_pos_usize..file_cursor_pos_usize + to_copy],
                        )?;
                        *file_cursor_pos = file_cursor_pos.checked_add(u64::try_from(to_copy)?)?;
                        debug_assert!(
                            *file_cursor_pos
                                <= u64::try_from(content.len()).unwrap_or(u64::max_value())
                        );
                        total_read += to_copy;
                    }
                    u32::try_from(total_read)?
                }
            }
        }
    };

    // Write to the last parameter the number of bytes that have been read in total.
    let out_ptr = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());
    mem_access.write_memory(out_ptr, &total_read.to_le_bytes())?;

    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

fn fd_seek(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let mut file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to seek.
    let mut file_descriptor = {
        let fd = usize::try_from(params.next().unwrap().into_i32().unwrap())?;
        match file_descriptors_lock.get_mut(fd).and_then(|v| v.as_mut()) {
            Some(fd) => fd,
            None => {
                let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return Ok((ContextInner::Finished, action));
            }
        }
    };

    let offset: i64 = params.next().unwrap().into_i64().unwrap();
    let whence = u8::try_from(params.next().unwrap().into_i32().unwrap())?;

    let new_offset: u64 = match &mut file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut { .. } => {
            // TODO: is that the correct error?
            let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
            let action = ExtrinsicsAction::Resume(ret);
            return Ok((ContextInner::Finished, action));
        }
        FileDescriptor::FilesystemEntry {
            inode,
            file_cursor_pos,
        } => {
            match &**inode {
                Inode::Directory { .. } => {
                    // TODO: is that the correct error?
                    let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                    let action = ExtrinsicsAction::Resume(ret);
                    return Ok((ContextInner::Finished, action));
                }
                Inode::File { content, .. } => {
                    let max_offset = u64::try_from(content.len())?;
                    // TODO: do that properly
                    let new_offset = match whence {
                        wasi::WHENCE_SET => {
                            cmp::min(u64::try_from(cmp::max(offset, 0))?, max_offset)
                        }
                        wasi::WHENCE_CUR => {
                            cmp::min(file_cursor_pos.saturating_add(offset as u64), max_offset)
                        }
                        wasi::WHENCE_END => cmp::min(
                            u64::try_from(cmp::max(0, max_offset as i64 + offset))?,
                            max_offset,
                        ),
                        _ => panic!(), // TODO: no
                    };
                    *file_cursor_pos = new_offset;
                    new_offset
                }
            }
        }
    };

    // Write to the last parameter the new offset.
    let out_ptr = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());
    mem_access.write_memory(out_ptr, &new_offset.to_le_bytes())?;

    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

fn fd_write(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let mut file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to write to.
    let fd = usize::try_from(params.next().unwrap().into_i32().unwrap())?;
    let file_descriptor = {
        match file_descriptors_lock.get_mut(fd).and_then(|v| v.as_mut()) {
            Some(fd) => fd,
            None => {
                let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return Ok((ContextInner::Finished, action));
            }
        }
    };

    // Get a list of pointers and lengths to write.
    // Elements 0, 2, 4, 6, ... in that list are pointers, and elements 1, 3, 5, 7, ... are
    // lengths.
    let list_to_write = {
        let addr = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
        let num = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
        let list_buf = mem_access.read_memory(addr..addr + 4 * num * 2)?;
        // TODO: don't panic if allocation size is too large
        let mut list_out = Vec::with_capacity(usize::try_from(num)?);
        for elem in list_buf.chunks(4) {
            list_out.push(u32::from_le_bytes(<[u8; 4]>::try_from(elem).unwrap()));
        }
        list_out
    };

    match file_descriptor {
        FileDescriptor::Empty => {
            // TODO: is that the right error code?
            let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_NOSYS)));
            let action = ExtrinsicsAction::Resume(ret);
            Ok((ContextInner::Finished, action))
        }
        FileDescriptor::LogOut { level, buffer } => {
            let mut total_written = 0usize;
            for ptr_and_len in list_to_write.chunks(2) {
                let ptr = ptr_and_len[0];
                let len = ptr_and_len[1];

                buffer.extend(mem_access.read_memory(ptr..ptr + len)?);
                total_written = total_written.checked_add(usize::try_from(len)?)?;
            }

            // Write to the fourth parameter the number of bytes written to the file descriptor.
            {
                let out_ptr = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
                let total_written = u32::try_from(total_written)?;
                mem_access.write_memory(out_ptr, &total_written.to_le_bytes())?;
            }

            assert!(params.next().is_none());

            // Flush `buffer` into a log message if possible.
            if let Some(split_pos) = buffer.iter().position(|c| *c == b'\n') {
                let mut encoded_message = Vec::new();
                encoded_message.push(u8::from(*level));
                encoded_message.extend(buffer.drain(..split_pos));
                buffer.remove(0);

                let action = ExtrinsicsAction::EmitMessage {
                    interface: redshirt_log_interface::ffi::INTERFACE,
                    message: EncodedMessage(encoded_message),
                    response_expected: false,
                };

                let context = ContextInner::TryFlushLogOut(fd);
                Ok((context, action))

            } else {
                let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
                return Ok((ContextInner::Finished, action));
            }
        }
        FileDescriptor::FilesystemEntry { .. } => unimplemented!(), // TODO:
    }
}

fn path_filestat_get(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let file_descriptors_lock = state.file_descriptors.lock();

    let file_descriptor = {
        let fd = usize::try_from(params.next().unwrap().into_i32().unwrap())?;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return Ok((ContextInner::Finished, action));
            }
        }
    };

    let fd_inode = match file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut { .. } => {
            // TODO: is that the correct return type?
            let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
            let action = ExtrinsicsAction::Resume(ret);
            return Ok((ContextInner::Finished, action));
        }
        FileDescriptor::FilesystemEntry { inode, .. } => inode.clone(),
    };

    let _lookup_flags = u32::try_from(params.next().unwrap().into_i32().unwrap())?;

    let path = {
        let path_buf = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
        let path_buf_len = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
        let path_utf8 = mem_access.read_memory(path_buf..path_buf + path_buf_len)?;
        String::from_utf8(path_utf8)? // TODO: return error code?
    };

    let resolved_path = match resolve_path(&fd_inode, &path) {
        Some(p) => p,
        None => {
            let action =
                ExtrinsicsAction::Resume(Some(WasmValue::I32(From::from(wasi::ERRNO_NOENT))));
            return Ok((ContextInner::Finished, action));
        }
    };

    let filestat = filestat_from_inode(&resolved_path);

    let filestat_out_buf = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    // Note: this is a bit of dark magic, but it is the only solution at the moment.
    // Can be tested with the following snippet:
    // ```c
    // #include <stdio.h>
    // #include <wasi/api.h>
    // int main() {
    //     __wasi_filestat_t* ptr = (__wasi_filestat_t*)0x1000;
    //     printf("%p %p %p %p %p %p %p %p %p %d\n", ptr, &ptr->dev, &ptr->ino, &ptr->filetype, &ptr->nlink, &ptr->size, &ptr->atim, &ptr->mtim, &ptr->ctim, sizeof(__wasi_filestat_t));
    //     return 0;
    // }
    // ```
    // Which prints `0x1000 0x1000 0x1008 0x1010 0x1018 0x1020 0x1028 0x1030 0x1038 64`
    mem_access.write_memory(filestat_out_buf, &[0; 64])?;
    mem_access.write_memory(filestat_out_buf, &filestat.dev.to_le_bytes())?;
    mem_access.write_memory(
        filestat_out_buf.checked_add(8)?,
        &filestat.ino.to_le_bytes(),
    )?;
    mem_access.write_memory(
        filestat_out_buf.checked_add(16)?,
        &filestat.filetype.to_le_bytes(),
    )?;
    mem_access.write_memory(
        filestat_out_buf.checked_add(24)?,
        &filestat.nlink.to_le_bytes(),
    )?;
    mem_access.write_memory(
        filestat_out_buf.checked_add(32)?,
        &filestat.size.to_le_bytes(),
    )?;
    mem_access.write_memory(
        filestat_out_buf.checked_add(40)?,
        &filestat.atim.to_le_bytes(),
    )?;
    mem_access.write_memory(
        filestat_out_buf.checked_add(48)?,
        &filestat.mtim.to_le_bytes(),
    )?;
    mem_access.write_memory(
        filestat_out_buf.checked_add(56)?,
        &filestat.ctim.to_le_bytes(),
    )?;

    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

fn path_open(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let mut file_descriptors_lock = state.file_descriptors.lock();

    let file_descriptor = {
        let fd = usize::try_from(params.next().unwrap().into_i32().unwrap())?;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return Ok((ContextInner::Finished, action));
            }
        }
    };

    let fd_inode = match file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut { .. } => {
            // TODO: is that the correct return type?
            let ret = Some(WasmValue::I32(From::from(wasi::ERRNO_BADF)));
            let action = ExtrinsicsAction::Resume(ret);
            return Ok((ContextInner::Finished, action));
        }
        FileDescriptor::FilesystemEntry { inode, .. } => inode.clone(),
    };

    let _lookup_flags = u32::try_from(params.next().unwrap().into_i32().unwrap())?;

    let path = {
        let path_buf = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
        let path_buf_len = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
        let path_utf8 = mem_access.read_memory(path_buf..path_buf + path_buf_len)?;
        String::from_utf8(path_utf8)? // TODO: return error code?
    };

    let resolved_path = match resolve_path(&fd_inode, &path) {
        Some(p) => p,
        None => {
            let action =
                ExtrinsicsAction::Resume(Some(WasmValue::I32(From::from(wasi::ERRNO_NOENT))));
            return Ok((ContextInner::Finished, action));
        }
    };

    let _open_flags = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    let _fs_rights_base = u64::try_from(params.next().unwrap().into_i64().unwrap())?;
    let _fs_rights_inherting = u64::try_from(params.next().unwrap().into_i64().unwrap())?;
    let _fd_flags = u32::try_from(params.next().unwrap().into_i32().unwrap())?;

    let new_fd = if let Some(fd_val) = file_descriptors_lock.iter().position(|fd| fd.is_none()) {
        file_descriptors_lock[fd_val] = Some(FileDescriptor::FilesystemEntry {
            inode: resolved_path,
            file_cursor_pos: 0,
        });
        // TODO: return error code with "too many fds"
        u32::try_from(fd_val).unwrap()
    } else {
        let fd_val = file_descriptors_lock.len();
        file_descriptors_lock.push(Some(FileDescriptor::FilesystemEntry {
            inode: resolved_path,
            file_cursor_pos: 0,
        }));
        // TODO: return error code with "too many fds"
        u32::try_from(fd_val).unwrap()
    };

    let opened_fd_ptr = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    mem_access.write_memory(opened_fd_ptr, &new_fd.to_le_bytes())?;

    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

fn poll_oneoff(
    _: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let _subscriptions_buf = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    let _events_out_buf = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    let _buf_size = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    let _num_events_out = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    unimplemented!()
}

fn proc_exit(
    _: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let _ret_val = params.next().unwrap().into_i32().unwrap();
    assert!(params.next().is_none());

    // TODO: returning `ProgramCrash` leads to `unimplemented!()`, so we panic
    // beforehand for a more explicit message
    // If the exit code is weird, it's probably one of these values:
    // https://github.com/WebAssembly/wasi-libc/blob/320054e84f8f2440def3b1c8700cedb8fd697bf8/libc-top-half/musl/include/sysexits.h
    panic!("proc_exit called with {:?}", _ret_val);

    // TODO: implement in a better way than crashing?
    Ok((ContextInner::Finished, ExtrinsicsAction::ProgramCrash))
}

fn random_get(
    _: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let buf = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    let len = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    let len_to_request = u16::try_from(len).unwrap_or(u16::max_value());
    debug_assert!(u32::from(len_to_request) <= len);
    let action = ExtrinsicsAction::EmitMessage {
        interface: redshirt_random_interface::ffi::INTERFACE,
        message: redshirt_random_interface::ffi::RandomMessage::Generate {
            len: len_to_request,
        }
        .encode(),
        response_expected: true,
    };

    let context = ContextInner::WaitRandom {
        out_ptr: buf,
        remaining_len: len,
    };

    Ok((context, action))
}

fn sched_yield(
    _: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    // TODO: implement in a better way?
    assert!(params.next().is_none());
    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

// Utility functions below.

fn args_or_env_get(
    list: &[Vec<u8>],
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let argv = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    let argv_buf = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    let mut argv_pos = 0;
    let mut argv_buf_pos = 0;

    for arg in list.iter() {
        mem_access.write_memory(
            argv.checked_add(argv_pos)?,
            &(argv_buf.checked_add(argv_buf_pos)?).to_le_bytes(),
        )?;
        argv_pos = argv_pos.checked_add(4)?;
        mem_access.write_memory(argv_buf.checked_add(argv_buf_pos)?, &arg)?;
        argv_buf_pos = argv_buf_pos.checked_add(u32::try_from(arg.len())?)?;
        mem_access.write_memory(argv_buf.checked_add(argv_buf_pos)?, &[0])?;
        argv_buf_pos = argv_buf_pos.checked_add(1)?;
    }

    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

fn args_or_env_sizes_get(
    list: &[Vec<u8>],
    mut params: impl ExactSizeIterator<Item = WasmValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> Result<(ContextInner, ExtrinsicsAction), WasiCallErr> {
    let argc_out = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    let argv_buf_size_out = u32::try_from(params.next().unwrap().into_i32().unwrap())?;
    assert!(params.next().is_none());

    mem_access.write_memory(argc_out, &u32::try_from(list.len())?.to_le_bytes())?;
    let argv_buf_size = list
        .iter()
        .fold(0usize, |s, a| s.saturating_add(a.len()).saturating_add(1));
    mem_access.write_memory(
        argv_buf_size_out,
        &u32::try_from(argv_buf_size)?.to_le_bytes(),
    )?;

    let action = ExtrinsicsAction::Resume(Some(WasmValue::I32(0)));
    Ok((ContextInner::Finished, action))
}

fn filestat_from_inode(inode: &Arc<Inode>) -> wasi::Filestat {
    wasi::Filestat {
        dev: 1,                                        // TODO:
        ino: &**inode as *const Inode as usize as u64, // TODO:
        filetype: match **inode {
            Inode::Directory { .. } => wasi::FILETYPE_DIRECTORY,
            Inode::File { .. } => wasi::FILETYPE_REGULAR_FILE,
        },
        nlink: 1, // TODO:
        size: match &**inode {
            Inode::Directory { .. } => 0,
            Inode::File { content } => {
                wasi::Filesize::try_from(content.len()).unwrap_or(wasi::Filesize::max_value())
            }
        },
        atim: 0, // TODO:
        mtim: 0, // TODO:
        ctim: 0, // TODO:
    }
}

fn resolve_path(root: &Arc<Inode>, path: &str) -> Option<Arc<Inode>> {
    let mut current = root.clone();

    for component in path.split('/') {
        if component == "." {
            continue;
        }

        if component == ".." {
            unimplemented!()
        }

        let next = match &*current {
            Inode::File { .. } => return None,
            Inode::Directory { entries } => {
                let entries = entries.lock();
                entries.get(component)?.clone()
            }
        };

        current = next;
    }

    Some(current)
}
