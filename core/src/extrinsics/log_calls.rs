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

//! Implementation of the [`Extrinsics`] trait that wraps around another implementation and sends
//! all the calls to the `log` interface for debugging.

use crate::extrinsics::{Extrinsics, ExtrinsicsAction, ExtrinsicsMemoryAccess, SupportedExtrinsic};
use crate::{EncodedMessage, ThreadId};

use alloc::{borrow::Cow, format, string::String, vec, vec::Vec};
use core::mem;
use wasmi::RuntimeValue;

/// Implementation of the [`Extrinsics`] trait that logs all calls to the underlying handler.
///
/// Every time a call has finished, a message is sent to the `log` interface containing the
/// parameters and return value.
///
/// This struct only has access to the low-level signatures of the functions it wraps around, and
/// therefore cannot, for example, print the content of memory that the inner function reads from
/// or writes to.
#[derive(Debug)]
pub struct LogExtrinsics<TInner> {
    /// Actual implementation.
    inner: TInner,
    /// Log level for the messages.
    log_level: redshirt_log_interface::Level,
}

impl<TInner> LogExtrinsics<TInner> {
    /// Builds a new [`LogExtrinsics`].
    pub fn new(inner: TInner) -> Self {
        LogExtrinsics {
            inner,
            log_level: redshirt_log_interface::Level::Trace,
        }
    }
}

impl<TInner> Default for LogExtrinsics<TInner>
where
    TInner: Default,
{
    fn default() -> Self {
        Self::new(Default::default())
    }
}

/// Identifier of an extrinsic.
#[derive(Debug, Clone)]
pub struct ExtrinsicId<TInner> {
    /// The function name prefixed with its module name.
    f_name: String,
    /// Actual identifier.
    inner: TInner,
}

/// Context for a logging call.
pub struct Context<TInner> {
    /// The inner context.
    inner: TInner,

    /// Start of the encoded log message. Only the return value is missing.
    message_start: Vec<u8>,

    /// If `Some`, the inner context has finished and we have sent a log message. When the
    /// confirmation comes back, we send back the content in this `Option`.
    waiting_for_log_message: Option<ExtrinsicsAction>,
}

/// Wraps around the inner iterator for supported extrinsics.
#[derive(Debug, Copy, Clone)]
pub struct LogIterator<TInner>(TInner);

impl<TInner> Extrinsics for LogExtrinsics<TInner>
where
    TInner: Extrinsics,
{
    type ExtrinsicId = ExtrinsicId<TInner::ExtrinsicId>;
    type Context = Context<TInner::Context>;
    type Iterator = LogIterator<TInner::Iterator>;

    fn supported_extrinsics() -> Self::Iterator {
        LogIterator(TInner::supported_extrinsics())
    }

    fn new_context(
        &self,
        thread_id: ThreadId,
        id: &Self::ExtrinsicId,
        params: impl ExactSizeIterator<Item = RuntimeValue>,
        mem_access: &mut impl ExtrinsicsMemoryAccess,
    ) -> (Self::Context, ExtrinsicsAction) {
        let params = params.collect::<Vec<_>>();

        let message_start = {
            let mut msg = vec![u8::from(self.log_level)];
            // TODO: print thread_id?
            msg.extend(id.f_name.as_bytes());
            msg.extend(b"(");
            for (n, param) in params.iter().enumerate() {
                if n != 0 {
                    msg.extend(b", ");
                }
                msg.extend(format!("{:?}", param).as_bytes());
            }
            msg.extend(b") -> ");
            msg
        };

        let (inner_ctxt, action) =
            self.inner
                .new_context(thread_id, &id.inner, params.into_iter(), mem_access);
        let mut ctxt = Context {
            inner: inner_ctxt,
            message_start,
            waiting_for_log_message: None,
        };

        let ret_value_str = match action {
            ExtrinsicsAction::Resume(val) => {
                ctxt.waiting_for_log_message = Some(ExtrinsicsAction::Resume(val));
                Cow::Owned(format!("{:?}", val).into_bytes())
            }
            a @ ExtrinsicsAction::ProgramCrash => {
                ctxt.waiting_for_log_message = Some(a);
                Cow::Borrowed(&b"<crash>"[..])
            }
            a @ ExtrinsicsAction::EmitMessage { .. } => return (ctxt, a),
        };

        let mut message = mem::replace(&mut ctxt.message_start, Vec::new());
        message.extend(&ret_value_str[..]);

        debug_assert!(ctxt.waiting_for_log_message.is_some());
        let action = ExtrinsicsAction::EmitMessage {
            interface: redshirt_log_interface::ffi::INTERFACE,
            message: EncodedMessage(message),
            response_expected: false,
        };

        (ctxt, action)
    }

    fn inject_message_response(
        &self,
        ctxt: &mut Self::Context,
        response: Option<EncodedMessage>,
        mem_access: &mut impl ExtrinsicsMemoryAccess,
    ) -> ExtrinsicsAction {
        if let Some(waiting_for_log_message) = ctxt.waiting_for_log_message.take() {
            debug_assert!(response.is_none());
            debug_assert!(ctxt.message_start.is_empty());
            return waiting_for_log_message;
        }

        let ret_value_str =
            match self
                .inner
                .inject_message_response(&mut ctxt.inner, response, mem_access)
            {
                ExtrinsicsAction::Resume(val) => {
                    ctxt.waiting_for_log_message = Some(ExtrinsicsAction::Resume(val));
                    Cow::Owned(format!("{:?}", val).into_bytes())
                }
                a @ ExtrinsicsAction::ProgramCrash => {
                    ctxt.waiting_for_log_message = Some(a);
                    Cow::Borrowed(&b"<crash>"[..])
                }
                a @ ExtrinsicsAction::EmitMessage { .. } => return a,
            };

        let mut message = mem::replace(&mut ctxt.message_start, Vec::new());
        message.extend(&ret_value_str[..]);

        debug_assert!(ctxt.waiting_for_log_message.is_some());
        ExtrinsicsAction::EmitMessage {
            interface: redshirt_log_interface::ffi::INTERFACE,
            message: EncodedMessage(message),
            response_expected: false,
        }
    }
}

impl<TInner, TExtId> Iterator for LogIterator<TInner>
where
    TInner: Iterator<Item = SupportedExtrinsic<TExtId>>,
{
    type Item = SupportedExtrinsic<ExtrinsicId<TExtId>>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.0.next()?;

        let id = ExtrinsicId {
            f_name: format!("{}::{}", item.wasm_interface, item.function_name),
            inner: item.id,
        };

        Some(SupportedExtrinsic {
            id,
            wasm_interface: item.wasm_interface,
            function_name: item.function_name,
            signature: item.signature,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<TInner, TExtId> ExactSizeIterator for LogIterator<TInner> where
    TInner: ExactSizeIterator<Item = SupportedExtrinsic<TExtId>>
{
}
