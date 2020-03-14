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

use crate::extrinsics::{Extrinsics, ExtrinsicsAction, ExtrinsicsMemoryAccess, SupportedExtrinsic};
use crate::{sig, Encode as _, EncodedMessage, ThreadId};

use alloc::{borrow::Cow, vec, vec::IntoIter};
use core::convert::TryFrom as _;
use wasmi::RuntimeValue;

/// Dummy implementation of the [`Extrinsics`] trait.
#[derive(Debug, Default)]
pub struct WasiExtrinsics;

/// Identifier of a WASI extrinsic.
#[derive(Debug, Clone)]
pub struct ExtrinsicId(ExtrinsicIdInner);

#[derive(Debug, Clone)]
enum ExtrinsicIdInner {
    ClockTimeGet,
    EnvironGet,
    EnvironSizesGet,
    FdWrite,
    ProcExit,
    RandomGet,
    SchedYield,
}

/// Context for a call to a WASI external function.
pub struct Context(ContextInner);

enum ContextInner {
    WaitRandom { out_ptr: u32, remaining_len: u16 },
    Finished,
}

impl Extrinsics for WasiExtrinsics {
    type ExtrinsicId = ExtrinsicId;
    type Context = Context;
    type Iterator = IntoIter<SupportedExtrinsic<Self::ExtrinsicId>>;

    fn supported_extrinsics() -> Self::Iterator {
        vec![
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
                id: ExtrinsicId(ExtrinsicIdInner::FdWrite),
                wasm_interface: Cow::Borrowed("wasi_snapshot_preview1"),
                function_name: Cow::Borrowed("fd_write"),
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
        mut params: impl ExactSizeIterator<Item = RuntimeValue>,
        mem_access: &mut impl ExtrinsicsMemoryAccess,
    ) -> (Self::Context, ExtrinsicsAction) {
        match id.0 {
            ExtrinsicIdInner::ClockTimeGet => unimplemented!(),
            ExtrinsicIdInner::EnvironGet => unimplemented!(),
            ExtrinsicIdInner::EnvironSizesGet => {
                let num_ptr = params.next().unwrap().try_into::<i32>().unwrap() as u32;
                let buf_size_ptr = params.next().unwrap().try_into::<i32>().unwrap() as u32;
                assert!(params.next().is_none());

                mem_access
                    .write_memory(num_ptr, &0u64.to_le_bytes())
                    .unwrap(); // TODO: don't unwrap
                mem_access
                    .write_memory(buf_size_ptr, &0u64.to_le_bytes())
                    .unwrap(); // TODO: don't unwrap

                let context = ContextInner::Finished;
                (
                    Context(context),
                    ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0))),
                )
            }
            ExtrinsicIdInner::FdWrite => unimplemented!(),
            ExtrinsicIdInner::ProcExit => {
                // TODO: implement in a better way?
                let context = ContextInner::Finished;
                (Context(context), ExtrinsicsAction::ProgramCrash)
            }
            ExtrinsicIdInner::RandomGet => {
                let buf = params.next().unwrap().try_into::<i32>().unwrap() as u32;
                let len = params.next().unwrap().try_into::<i32>().unwrap();
                assert!(params.next().is_none());

                let len_to_request = u16::try_from(len).unwrap_or(u16::max_value());
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
                    remaining_len: len_to_request,
                };

                (Context(context), action)
            }
            ExtrinsicIdInner::SchedYield => {
                // TODO: implement in a better way?
                let context = ContextInner::Finished;
                (
                    Context(context),
                    ExtrinsicsAction::Resume(Some(RuntimeValue::I32(0))),
                )
            }
        }
    }

    fn inject_message_response(
        &self,
        ctxt: &mut Self::Context,
        _: Option<EncodedMessage>,
        _: &mut impl ExtrinsicsMemoryAccess,
    ) -> ExtrinsicsAction {
        match ctxt.0 {
            ContextInner::WaitRandom { .. } => unimplemented!(),
            ContextInner::Finished => unreachable!(),
        }
    }
}
