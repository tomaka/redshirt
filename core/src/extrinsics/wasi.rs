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
use crate::{sig, Encode as _, EncodedMessage, ThreadId};

use alloc::{
    borrow::Cow,
    string::{String, ToString as _},
    sync::Arc,
    vec,
    vec::{IntoIter, Vec},
};
use core::{cmp, convert::TryFrom as _, mem, slice};
use hashbrown::HashMap;
use spin::Mutex;
use wasmi::RuntimeValue;

/// Dummy implementation of the [`Extrinsics`] trait.
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
    file_descriptors: Mutex<Vec<Option<FileDescriptor>>>,

    /// Virtual file system accessible to the program.
    file_system: Arc<Inode>,
}

#[derive(Debug)]
enum FileDescriptor {
    /// Valid file descriptor but that points to nothing.
    Empty,
    LogOut(redshirt_log_interface::Level),
    FilesystemEntry {
        inode: Arc<Inode>,
        /// Position of the cursor within the file. Always 0 for directories.
        file_cursor_pos: u64,
    },
}

#[derive(Debug)]
enum Inode {
    Directory {
        entries: Mutex<HashMap<String, Arc<Inode>, fnv::FnvBuildHasher>>,
    },
    File {
        content: Vec<u8>,
    },
}

impl Default for WasiExtrinsics {
    fn default() -> WasiExtrinsics {
        let fs_root = Arc::new(Inode::Directory {
            entries: Mutex::new({
                let mut hashmap = HashMap::default();
                // TODO: hack to toy with DOOM
                /*hashmap.insert(
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
            file_descriptors: Mutex::new(vec![
                // stdin
                Some(FileDescriptor::Empty),
                // stdout
                Some(FileDescriptor::LogOut(redshirt_log_interface::Level::Info)),
                // stderr
                Some(FileDescriptor::LogOut(redshirt_log_interface::Level::Error)),
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
    Resume(Option<RuntimeValue>),
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
        params: impl ExactSizeIterator<Item = RuntimeValue>,
        mem_access: &mut impl ExtrinsicsMemoryAccess,
    ) -> (Self::Context, ExtrinsicsAction) {
        let (context, action) = match id.0 {
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
            ExtrinsicIdInner::FdWrite => fd_write(self, params, mem_access),
            ExtrinsicIdInner::PathCreateDirectory => unimplemented!(),
            ExtrinsicIdInner::PathFilestatGet => path_filestat_get(self, params, mem_access),
            ExtrinsicIdInner::PathOpen => path_open(self, params, mem_access),
            ExtrinsicIdInner::PollOneOff => poll_oneoff(self, params, mem_access),
            ExtrinsicIdInner::ProcExit => proc_exit(self, params, mem_access),
            ExtrinsicIdInner::RandomGet => random_get(self, params, mem_access),
            ExtrinsicIdInner::SchedYield => sched_yield(self, params, mem_access),
        };

        (Context(context), action)
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
                ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)))
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
                    ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)))
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
            ContextInner::Resume(value) => {
                ctxt.0 = ContextInner::Finished;
                ExtrinsicsAction::Resume(value)
            }
            ContextInner::Finished => unreachable!(),
        }
    }
}

// Implementations of WASI function calls below.

fn args_get(
    state: &WasiExtrinsics,
    params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    args_or_env_get(&state.args, params, mem_access)
}

fn args_sizes_get(
    state: &WasiExtrinsics,
    params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    args_or_env_sizes_get(&state.args, params, mem_access)
}

fn clock_time_get(
    _: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let clock_id = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    let _precision = params.next().unwrap().try_into::<i64>().unwrap();

    let time_out = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());

    match clock_id {
        // TODO: as a hack for now handle REALTIME the same as MONOTONIC
        wasi::CLOCKID_REALTIME | wasi::CLOCKID_MONOTONIC => {
            let action = ExtrinsicsAction::EmitMessage {
                interface: redshirt_time_interface::ffi::INTERFACE,
                message: redshirt_time_interface::ffi::TimeMessage::GetMonotonic.encode(),
                response_expected: true,
            };

            let context = ContextInner::WaitClockVal { out_ptr: time_out };

            (context, action)
        }
        wasi::CLOCKID_PROCESS_CPUTIME_ID => unimplemented!(),
        wasi::CLOCKID_THREAD_CPUTIME_ID => unimplemented!(),
        _ => panic!(),
    }
}

fn environ_get(
    state: &WasiExtrinsics,
    params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    args_or_env_get(&state.env_vars, params, mem_access)
}

fn environ_sizes_get(
    state: &WasiExtrinsics,
    params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    args_or_env_sizes_get(&state.env_vars, params, mem_access)
}

fn fd_close(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let mut file_descriptors_lock = state.file_descriptors.lock();

    let fd = params.next().unwrap().try_into::<i32>().unwrap() as usize;
    assert!(params.next().is_none());

    // Check validity of the file descriptor.
    if file_descriptors_lock
        .get(fd)
        .map(|f| f.is_none())
        .unwrap_or(true)
    {
        let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
        let action = ExtrinsicsAction::Resume(ret);
        return (ContextInner::Finished, action);
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

    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
}

fn fd_fdstat_get(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to write to.
    let file_descriptor = {
        let fd = params.next().unwrap().try_into::<i32>().unwrap() as usize;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return (ContextInner::Finished, action);
            }
        }
    };

    let dirs_rights = wasi::RIGHTS_PATH_OPEN | wasi::RIGHTS_FD_READDIR;
    let files_rights = wasi::RIGHTS_FD_READ | wasi::RIGHTS_FD_SEEK | wasi::RIGHTS_FD_TELL;

    let stat = match file_descriptor {
        FileDescriptor::Empty => wasi::Fdstat {
            fs_filetype: wasi::FILETYPE_CHARACTER_DEVICE,
            fs_flags: 0,
            fs_rights_base: 0,
            fs_rights_inheriting: 0,
        },
        FileDescriptor::LogOut(_) => wasi::Fdstat {
            fs_filetype: wasi::FILETYPE_CHARACTER_DEVICE,
            fs_flags: wasi::FDFLAGS_APPEND,
            fs_rights_base: wasi::RIGHTS_FD_WRITE,
            fs_rights_inheriting: wasi::RIGHTS_FD_WRITE,
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

    let stat_out_buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());

    // TODO: no unsafe
    unsafe {
        mem_access
            .write_memory(
                stat_out_buf,
                slice::from_raw_parts(
                    &stat as *const wasi::Fdstat as *const u8,
                    mem::size_of::<wasi::Fdstat>(),
                ),
            )
            .unwrap(); // TODO: don't unwrap
    }

    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
}

fn fd_filestat_get(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to write to.
    let file_descriptor = {
        let fd = params.next().unwrap().try_into::<i32>().unwrap() as usize;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return (ContextInner::Finished, action);
            }
        }
    };

    unimplemented!();

    let _stat_out_buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());

    // Returning `__WASI_ERRNO_BADF` all the time.
    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF))));
    (ContextInner::Finished, action)
}

fn fd_prestat_dir_name(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to write to.
    let file_descriptor = {
        let fd = params.next().unwrap().try_into::<i32>().unwrap() as usize;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return (ContextInner::Finished, action);
            }
        }
    };

    let name = match file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut(_) => {
            // TODO: is that the correct return type?
            let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
            let action = ExtrinsicsAction::Resume(ret);
            return (ContextInner::Finished, action);
        }
        FileDescriptor::FilesystemEntry { .. } => b"hello\0", // TODO:
    };

    let path_out = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    let path_out_len = params.next().unwrap().try_into::<i32>().unwrap() as usize;
    assert!(params.next().is_none());

    // TODO: is it correct to truncate if the buffer is too small?
    // TODO: also, do we need a null terminator?
    let to_write = cmp::min(path_out_len, name.len());
    // TODO: don't unwrap
    mem_access
        .write_memory(path_out, &name[..to_write])
        .unwrap();

    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
}

fn fd_prestat_get(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to write to.
    let file_descriptor = {
        let fd = params.next().unwrap().try_into::<i32>().unwrap() as usize;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return (ContextInner::Finished, action);
            }
        }
    };

    let prestat = match file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut(_) => {
            // TODO: is that the correct return type?
            let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
            let action = ExtrinsicsAction::Resume(ret);
            return (ContextInner::Finished, action);
        }
        FileDescriptor::FilesystemEntry { inode, .. } => match **inode {
            // TODO: we don't know for sure that it's been pre-open
            Inode::Directory { .. } => wasi::Prestat {
                pr_type: wasi::PREOPENTYPE_DIR,
                u: wasi::PrestatU {
                    dir: wasi::PrestatDir {
                        pr_name_len: 6, // TODO:
                    },
                },
            },
            Inode::File { .. } => {
                // TODO: is that the correct return type?
                let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return (ContextInner::Finished, action);
            }
        },
    };

    let prestat_out_buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());

    // TODO: no unsafe
    unsafe {
        mem_access
            .write_memory(
                prestat_out_buf,
                slice::from_raw_parts(
                    &prestat as *const wasi::Prestat as *const u8,
                    mem::size_of::<wasi::Prestat>(),
                ),
            )
            .unwrap(); // TODO: don't unwrap
    }

    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
}

fn fd_read(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let mut file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to read from.
    let mut file_descriptor = {
        let fd = params.next().unwrap().try_into::<i32>().unwrap() as usize;
        match file_descriptors_lock.get_mut(fd).and_then(|v| v.as_mut()) {
            Some(fd) => fd,
            None => {
                let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return (ContextInner::Finished, action);
            }
        }
    };

    // Get a list of pointers and lengths to read to.
    // Elements 0, 2, 4, 6, ... in that list are pointers, and elements 1, 3, 5, 7, ... are
    // lengths.
    let out_buffers_list = {
        let addr = params.next().unwrap().try_into::<i32>().unwrap() as u32;
        let num = params.next().unwrap().try_into::<i32>().unwrap() as u32;
        let list_buf = mem_access.read_memory(addr..addr + 4 * num * 2).unwrap();
        let mut list_out = Vec::with_capacity(usize::try_from(num).unwrap());
        for elem in list_buf.chunks(4) {
            list_out.push(u32::from_le_bytes(<[u8; 4]>::try_from(elem).unwrap()));
        }
        list_out
    };

    let total_read: u32 = match &mut file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut(_) => 0,
        FileDescriptor::FilesystemEntry {
            inode,
            file_cursor_pos,
        } => {
            match &**inode {
                Inode::Directory { .. } => {
                    // TODO: is that the correct error?
                    let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                    let action = ExtrinsicsAction::Resume(ret);
                    return (ContextInner::Finished, action);
                }
                Inode::File { content, .. } => {
                    let mut total_read = 0;
                    for buffer in out_buffers_list.chunks(2) {
                        let buffer_ptr = buffer[0];
                        let buffer_len = buffer[1] as usize;
                        let file_cursor_pos_usize = usize::try_from(*file_cursor_pos).unwrap();
                        let to_copy = cmp::min(content.len() - file_cursor_pos_usize, buffer_len);
                        if to_copy == 0 {
                            break;
                        }
                        mem_access
                            .write_memory(
                                buffer_ptr,
                                &content[file_cursor_pos_usize..file_cursor_pos_usize + to_copy],
                            )
                            .unwrap();
                        *file_cursor_pos += u64::try_from(to_copy).unwrap();
                        total_read += to_copy;
                    }
                    u32::try_from(total_read).unwrap()
                }
            }
        }
    };

    // Write to the last parameter the number of bytes that have been read in total.
    let out_ptr = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());
    mem_access
        .write_memory(out_ptr, &total_read.to_le_bytes())
        .unwrap();

    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
}

fn fd_seek(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let mut file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to seek.
    let mut file_descriptor = {
        let fd = params.next().unwrap().try_into::<i32>().unwrap() as usize;
        match file_descriptors_lock.get_mut(fd).and_then(|v| v.as_mut()) {
            Some(fd) => fd,
            None => {
                let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return (ContextInner::Finished, action);
            }
        }
    };

    let offset = params.next().unwrap().try_into::<i64>().unwrap();
    let whence = u8::try_from(params.next().unwrap().try_into::<i32>().unwrap()).unwrap();

    let new_offset: u64 = match &mut file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut(_) => {
            // TODO: is that the correct error?
            let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
            let action = ExtrinsicsAction::Resume(ret);
            return (ContextInner::Finished, action);
        }
        FileDescriptor::FilesystemEntry {
            inode,
            file_cursor_pos,
        } => {
            match &**inode {
                Inode::Directory { .. } => {
                    // TODO: is that the correct error?
                    let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                    let action = ExtrinsicsAction::Resume(ret);
                    return (ContextInner::Finished, action);
                }
                Inode::File { content, .. } => {
                    let max_offset = u64::try_from(content.len()).unwrap();
                    // TODO: do that properly
                    let new_offset = match whence {
                        wasi::WHENCE_SET => {
                            cmp::min(u64::try_from(cmp::max(offset, 0)).unwrap(), max_offset)
                        }
                        wasi::WHENCE_CUR => {
                            cmp::min(file_cursor_pos.saturating_add(offset as u64), max_offset)
                        }
                        wasi::WHENCE_END => cmp::min(
                            u64::try_from(cmp::max(0, max_offset as i64 + offset)).unwrap(),
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
    let out_ptr = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());
    mem_access
        .write_memory(out_ptr, &new_offset.to_le_bytes())
        .unwrap();

    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
}

fn fd_write(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let file_descriptors_lock = state.file_descriptors.lock();

    // Find out which file descriptor the user wants to write to.
    let file_descriptor = {
        let fd = params.next().unwrap().try_into::<i32>().unwrap() as usize;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return (ContextInner::Finished, action);
            }
        }
    };

    // Get a list of pointers and lengths to write.
    // Elements 0, 2, 4, 6, ... in that list are pointers, and elements 1, 3, 5, 7, ... are
    // lengths.
    let list_to_write = {
        let addr = params.next().unwrap().try_into::<i32>().unwrap() as u32;
        let num = params.next().unwrap().try_into::<i32>().unwrap() as u32;
        let list_buf = mem_access.read_memory(addr..addr + 4 * num * 2).unwrap();
        let mut list_out = Vec::with_capacity(usize::try_from(num).unwrap());
        for elem in list_buf.chunks(4) {
            list_out.push(u32::from_le_bytes(<[u8; 4]>::try_from(elem).unwrap()));
        }
        list_out
    };

    match file_descriptor {
        FileDescriptor::Empty => {
            // TODO: is that the right error code?
            let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOSYS)));
            let action = ExtrinsicsAction::Resume(ret);
            (ContextInner::Finished, action)
        }
        FileDescriptor::LogOut(log_level) => {
            let mut total_written = 0;
            let mut encoded_message = Vec::new();
            encoded_message.push(u8::from(*log_level));

            for ptr_and_len in list_to_write.chunks(2) {
                let ptr = ptr_and_len[0] as u32;
                let len = ptr_and_len[1] as u32;

                encoded_message.extend(mem_access.read_memory(ptr..ptr + len).unwrap());
                total_written += len as usize;
            }

            debug_assert_eq!(encoded_message.len(), total_written + 1);

            // Write to the fourth parameter the number of bytes written to the file descriptor.
            {
                let out_ptr = params.next().unwrap().try_into::<i32>().unwrap() as u32;
                let total_written = u32::try_from(total_written).unwrap();
                mem_access
                    .write_memory(out_ptr, &total_written.to_le_bytes())
                    .unwrap();
            }

            assert!(params.next().is_none());

            let action = ExtrinsicsAction::EmitMessage {
                interface: redshirt_log_interface::ffi::INTERFACE,
                message: EncodedMessage(encoded_message),
                response_expected: false,
            };

            let context = ContextInner::Resume(Some(RuntimeValue::I32(0)));
            (context, action)
        }
        FileDescriptor::FilesystemEntry { .. } => unimplemented!(),
    }
}

fn path_filestat_get(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let file_descriptors_lock = state.file_descriptors.lock();

    let file_descriptor = {
        let fd = params.next().unwrap().try_into::<i32>().unwrap() as usize;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return (ContextInner::Finished, action);
            }
        }
    };

    let fd_inode = match file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut(_) => {
            // TODO: is that the correct return type?
            let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
            let action = ExtrinsicsAction::Resume(ret);
            return (ContextInner::Finished, action);
        }
        FileDescriptor::FilesystemEntry { inode, .. } => inode.clone(),
    };

    let _lookup_flags = params.next().unwrap().try_into::<i32>().unwrap() as u32;

    let path = {
        let path_buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
        let path_buf_len = params.next().unwrap().try_into::<i32>().unwrap() as u32;
        // TODO: don't unwrap below
        let path_utf8 = mem_access
            .read_memory(path_buf..path_buf + path_buf_len)
            .unwrap();
        String::from_utf8(path_utf8).unwrap()
    };

    let resolved_path = match resolve_path(&fd_inode, &path) {
        Some(p) => p,
        None => {
            let action =
                ExtrinsicsAction::Resume(Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOENT))));
            return (ContextInner::Finished, action);
        }
    };

    let filestat = filestat_from_inode(&resolved_path);

    let filestat_out_buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());

    // TODO: no unsafe
    unsafe {
        mem_access
            .write_memory(
                filestat_out_buf,
                slice::from_raw_parts(
                    &filestat as *const wasi::Filestat as *const u8,
                    mem::size_of::<wasi::Filestat>(),
                ),
            )
            .unwrap(); // TODO: don't unwrap
    }

    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
}

fn path_open(
    state: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let mut file_descriptors_lock = state.file_descriptors.lock();

    let file_descriptor = {
        let fd = params.next().unwrap().try_into::<i32>().unwrap() as usize;
        match file_descriptors_lock.get(fd).and_then(|v| v.as_ref()) {
            Some(fd) => fd,
            None => {
                let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
                let action = ExtrinsicsAction::Resume(ret);
                return (ContextInner::Finished, action);
            }
        }
    };

    let fd_inode = match file_descriptor {
        FileDescriptor::Empty | FileDescriptor::LogOut(_) => {
            // TODO: is that the correct return type?
            let ret = Some(RuntimeValue::I32(From::from(wasi::ERRNO_BADF)));
            let action = ExtrinsicsAction::Resume(ret);
            return (ContextInner::Finished, action);
        }
        FileDescriptor::FilesystemEntry { inode, .. } => inode.clone(),
    };

    let _lookup_flags = params.next().unwrap().try_into::<i32>().unwrap() as u32;

    let path = {
        let path_buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
        let path_buf_len = params.next().unwrap().try_into::<i32>().unwrap() as u32;
        // TODO: don't unwrap below
        let path_utf8 = mem_access
            .read_memory(path_buf..path_buf + path_buf_len)
            .unwrap();
        String::from_utf8(path_utf8).unwrap()
    };

    let resolved_path = match resolve_path(&fd_inode, &path) {
        Some(p) => p,
        None => {
            let action =
                ExtrinsicsAction::Resume(Some(RuntimeValue::I32(From::from(wasi::ERRNO_NOENT))));
            return (ContextInner::Finished, action);
        }
    };

    let _open_flags = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    let _fs_rights_base = params.next().unwrap().try_into::<i64>().unwrap() as u64;
    let _fs_rights_inherting = params.next().unwrap().try_into::<i64>().unwrap() as u64;
    let _fd_flags = params.next().unwrap().try_into::<i32>().unwrap() as u32;

    let new_fd = if let Some(fd_val) = file_descriptors_lock.iter().position(|fd| fd.is_none()) {
        file_descriptors_lock[fd_val] = Some(FileDescriptor::FilesystemEntry {
            inode: resolved_path,
            file_cursor_pos: 0,
        });
        u32::try_from(fd_val).unwrap()
    } else {
        let fd_val = file_descriptors_lock.len();
        file_descriptors_lock.push(Some(FileDescriptor::FilesystemEntry {
            inode: resolved_path,
            file_cursor_pos: 0,
        }));
        u32::try_from(fd_val).unwrap()
    };

    let opened_fd_ptr = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());

    // TODO: don't unwrap
    mem_access
        .write_memory(opened_fd_ptr, &new_fd.to_le_bytes())
        .unwrap();

    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
}

fn poll_oneoff(
    _: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let _subscriptions_buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    let _events_out_buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    let _buf_size = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    let _num_events_out = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());

    unimplemented!()
}

fn proc_exit(
    _: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let _ret_val = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());

    // TODO: returning `ProgramCrash` leads to `unimplemented!()`, so we panic
    // beforehand for a more explicit message
    // If the exit code is weird, it's probably one of these values:
    // https://github.com/WebAssembly/wasi-libc/blob/320054e84f8f2440def3b1c8700cedb8fd697bf8/libc-top-half/musl/include/sysexits.h
    panic!("proc_exit called with {:?}", _ret_val);

    // TODO: implement in a better way than crashing?
    (ContextInner::Finished, ExtrinsicsAction::ProgramCrash)
}

fn random_get(
    _: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    let len = params.next().unwrap().try_into::<i32>().unwrap() as u32;
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

    (context, action)
}

fn sched_yield(
    _: &WasiExtrinsics,
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    _: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    // TODO: implement in a better way?
    assert!(params.next().is_none());
    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
}

// Utility functions below.

fn args_or_env_get(
    list: &[Vec<u8>],
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let argv = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    let argv_buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());

    let mut argv_pos = 0;
    let mut argv_buf_pos = 0;

    for arg in list.iter() {
        mem_access
            .write_memory(argv + argv_pos, &(argv_buf + argv_buf_pos).to_le_bytes())
            .unwrap(); // TODO: don't unwrap
        argv_pos += 4;
        mem_access
            .write_memory(argv_buf + argv_buf_pos, &arg)
            .unwrap(); // TODO: don't unwrap
        argv_buf_pos += u32::try_from(arg.len()).unwrap();
        mem_access
            .write_memory(argv_buf + argv_buf_pos, &[0])
            .unwrap(); // TODO: don't unwrap
        argv_buf_pos += 1;
    }

    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
}

fn args_or_env_sizes_get(
    list: &[Vec<u8>],
    mut params: impl ExactSizeIterator<Item = RuntimeValue>,
    mem_access: &mut impl ExtrinsicsMemoryAccess,
) -> (ContextInner, ExtrinsicsAction) {
    let argc_out = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    let argv_buf_size_out = params.next().unwrap().try_into::<i32>().unwrap() as u32;
    assert!(params.next().is_none());

    mem_access
        .write_memory(argc_out, &u32::try_from(list.len()).unwrap().to_le_bytes())
        .unwrap(); // TODO: don't unwrap
    let argv_buf_size = list.iter().fold(0, |s, a| s + a.len() + 1);
    mem_access
        .write_memory(
            argv_buf_size_out,
            &u32::try_from(argv_buf_size).unwrap().to_le_bytes(),
        )
        .unwrap(); // TODO: don't unwrap

    let action = ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0)));
    (ContextInner::Finished, action)
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
