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

use crate::{EncodedMessage, InterfaceHash, MessageId, ThreadId};
use crate::signature::Signature;

use alloc::borrow::Cow;
use core::iter;
use wasmi::RuntimeValue;

/// Trait implemented on types that can handle extrinsics.
///
/// The `Default` trait is used to instantiate structs that implement this trait. One instance is
/// created for each WASM process.
pub trait Extrinsics: Default {
    /// Identifier for an extrinsic function.
    ///
    /// Instead of passing around function names, we pass around identifiers.
    type ExtrinsicId: Send;
    type Context: Send;
    type Iterator: Iterator<Item = SupportedExtrinsic<Self::ExtrinsicId>>;

    /// Returns an iterator to the list of extrinsics that this struct supports.
    fn supported_extrinsics() -> Self::Iterator;

    fn new_context(&self, tid: ThreadId, id: &Self::ExtrinsicId, params: &[RuntimeValue]) -> Self::Context;

    fn poll(&self, ctxt: &mut Self::Context) -> ExtrinsicsAction;

    fn inject_message_response(&self, ctxt: &mut Self::Context, msg_id: MessageId, response: EncodedMessage);
}

pub struct SupportedExtrinsic<TExtId> {
    pub id: TExtId,
    pub wasm_interface: Cow<'static, str>,
    pub function_name: Cow<'static, str>,
    pub signature: Signature,
}

#[derive(Debug, Clone)]
pub enum ExtrinsicsAction {
    ProgramCrash,
    Resume(Option<RuntimeValue>),
    EmitMessage {
        interface: InterfaceHash,
        message: EncodedMessage,
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

    fn new_context(&self, _: ThreadId, id: &Self::ExtrinsicId, _: &[RuntimeValue]) -> Self::Context {
        match *id {} // unreachable
    }

    fn poll(&self, ctxt: &mut Self::Context) -> ExtrinsicsAction {
        match *ctxt {} // unreachable
    }

    fn inject_message_response(&self, ctxt: &mut Self::Context, _: MessageId, _: EncodedMessage) {
        match *ctxt {} // unreachable
    }
}
