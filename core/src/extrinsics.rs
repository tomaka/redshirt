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

//! Provides a way to implement custom extrinsics.
//!
//! The [`Extrinsics`] trait can be implemented on types that represent a collection of functions
//! that can be called by a WASM module.
//!
//! A function that is available to a WASM module is called an **extrinsic**.
//!
//! TODO: write doc on how to implement this trait

use crate::signature::Signature;
use crate::{EncodedMessage, InterfaceHash, ThreadId};

use alloc::{borrow::Cow, vec::Vec};
use core::{fmt, iter, ops::Range};
use wasmi::RuntimeValue;

pub mod log_calls;
pub mod wasi;

/// Trait implemented on types that can handle extrinsics.
///
/// The `Default` trait is used to instantiate structs that implement this trait. One instance is
/// created for each WASM process.
// TODO: in this API one can only emit one message at the time; this is fine in terms of logic, but
// is sub-optimal
pub trait Extrinsics: Default {
    /// Identifier for an extrinsic function.
    ///
    /// Instead of passing around function names, we pass around identifiers.
    ///
    /// This is typically an `enum` or an integer.
    type ExtrinsicId: Send;

    /// Created when an extrinsic is called by a WASM module.
    type Context: Send;

    /// Iterator returned by [`Extrinsics::supported_extrinsics`].
    type Iterator: Iterator<Item = SupportedExtrinsic<Self::ExtrinsicId>>;

    /// Returns an iterator to the list of extrinsics that this struct supports.
    fn supported_extrinsics() -> Self::Iterator;

    /// Called when a WASM module calls an extrinsic.
    ///
    /// Returns what to do next on this context.
    ///
    /// Returning [`ExtrinsicAction::Resume`] or [`ExtrinsicAction::ProgramCrash`] finishes the
    /// extrinsic call and destroys the context.
    fn new_context(
        &self,
        tid: ThreadId,
        id: &Self::ExtrinsicId,
        params: impl ExactSizeIterator<Item = RuntimeValue>,
        proc_access: &mut impl ExtrinsicsMemoryAccess,
    ) -> (Self::Context, ExtrinsicsAction);

    /// If [`ExtrinsicAction::EmitMessage`] has been emitted, this function is later called in
    /// order to notify of the response.
    ///
    /// The response is `None` if no response is expected.
    ///
    /// Returns what to do next on this context.
    ///
    /// Returning [`ExtrinsicAction::Resume`] or [`ExtrinsicAction::ProgramCrash`] finishes the
    /// extrinsic call and destroys the context.
    fn inject_message_response(
        &self,
        ctxt: &mut Self::Context,
        response: Option<EncodedMessage>,
        proc_access: &mut impl ExtrinsicsMemoryAccess,
    ) -> ExtrinsicsAction;
}

/// Access to a process's memory.
pub trait ExtrinsicsMemoryAccess {
    /// Reads the process' memory in the given range and returns a copy of it.
    ///
    /// # Panic
    ///
    /// A panic can occur if the start of the range is superior to its end. Note, however, that
    /// zero-sized ranges are allowed.
    // TODO: zero-cost API
    fn read_memory(&self, range: Range<u32>) -> Result<Vec<u8>, ExtrinsicsMemoryAccessErr>;

    /// Writes the given data in the process's memory at the given offset.
    fn write_memory(&mut self, offset: u32, data: &[u8]) -> Result<(), ExtrinsicsMemoryAccessErr>;
}

/// Error that can happen when reading or writing the memory of a process.
#[derive(Debug)]
pub enum ExtrinsicsMemoryAccessErr {
    /// The range is outside of the memory allocated to this process.
    OutOfRange,
}

impl fmt::Display for ExtrinsicsMemoryAccessErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ExtrinsicsMemoryAccessErr::OutOfRange => write!(
                f,
                "The range is outside of the memory allocated to this process."
            ),
        }
    }
}

/// Description of an extrinsic supported by the implementation of [`Extrinsics`].
#[derive(Debug)]
pub struct SupportedExtrinsic<TExtId> {
    /// Identifier of the extrinsic. Passed to [`new_context`] when the extrinsic is called.
    pub id: TExtId,

    /// Name of the interface the function belongs to.
    ///
    /// In WASM, each function belongs to what is called an interface. An interface can be summed
    /// up as the namespace of the function. This is unrelated to the concept of "interface"
    /// proper to redshirt.
    pub wasm_interface: Cow<'static, str>,

    /// Name of the function.
    pub function_name: Cow<'static, str>,

    /// Signature of the function. The parameters passed to [`new_context`] are guaranteed to
    /// match this signature.
    pub signature: Signature,
}

/// Action to perform in the context of an extrinsic being called.
#[derive(Debug, Clone)]
pub enum ExtrinsicsAction {
    /// Crash the program that called the extrinsic.
    ProgramCrash,

    /// Successfully finish the call and return with the given value.
    Resume(Option<RuntimeValue>),

    /// Emit a message.
    ///
    /// This makes it as if the process had emitted a message on the given interface, except that
    /// the response is later injected back using [`Extrinsics::inject_message_response`].
    EmitMessage {
        interface: InterfaceHash,
        message: EncodedMessage,
        response_expected: bool,
    },
}

/// Dummy implementation of the [`Extrinsics`] trait.
#[derive(Debug, Default)]
pub struct NoExtrinsics;

impl Extrinsics for NoExtrinsics {
    type ExtrinsicId = core::convert::Infallible; // TODO: `!` instead
    type Context = core::convert::Infallible; // TODO: `!` instead
    type Iterator = iter::Empty<SupportedExtrinsic<Self::ExtrinsicId>>;

    fn supported_extrinsics() -> Self::Iterator {
        iter::empty()
    }

    fn new_context(
        &self,
        _: ThreadId,
        id: &Self::ExtrinsicId,
        _: impl ExactSizeIterator<Item = RuntimeValue>,
        _: &mut impl ExtrinsicsMemoryAccess,
    ) -> (Self::Context, ExtrinsicsAction) {
        match *id {} // unreachable
    }

    fn inject_message_response(
        &self,
        ctxt: &mut Self::Context,
        _: Option<EncodedMessage>,
        _: &mut impl ExtrinsicsMemoryAccess,
    ) -> ExtrinsicsAction {
        match *ctxt {} // unreachable
    }
}
