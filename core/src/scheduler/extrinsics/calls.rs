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

//! Helpers for parsing the hardcoded functions that can be called by the WASM program.

use crate::scheduler::processes;
use crate::{InterfaceHash, InvalidMessageIdErr, MessageId};

use alloc::vec::Vec;
use core::{convert::TryFrom as _, num::NonZeroU64};
use redshirt_syscalls::EncodedMessage;

/// Analyzes a call to `next_notification` made by the given thread.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
pub fn parse_extrinsic_next_notification<TExtr, TPud, TTud>(
    thread: &mut processes::ThreadAccess<TExtr, TPud, TTud>,
    params: Vec<crate::WasmValue>,
) -> Result<NotificationWait, ExtrinsicNextNotificationErr> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 5);

    let notifs_ids_ptr = u32::try_from(
        params[0]
            .into_i32()
            .ok_or(ExtrinsicNextNotificationErr::BadParameter)?,
    )
    .map_err(|_| ExtrinsicNextNotificationErr::BadParameter)?;
    // TODO: consider not copying the notification ids and read memory on demand instead
    let notifs_ids = {
        let len = u32::try_from(
            params[1]
                .into_i32()
                .ok_or(ExtrinsicNextNotificationErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicNextNotificationErr::BadParameter)?;
        if len >= 512 {
            // TODO: arbitrary limit in order to not allocate too much memory below; a bit crappy
            return Err(ExtrinsicNextNotificationErr::TooManyNotificationIds { requested: len });
        }
        let mem = thread
            .read_memory(notifs_ids_ptr, len * 8)
            .map_err(|_| ExtrinsicNextNotificationErr::BadParameter)?;
        let len_usize = usize::try_from(len)
            .map_err(|_| ExtrinsicNextNotificationErr::TooManyNotificationIds { requested: len })?;
        let mut out = Vec::with_capacity(len_usize);
        for i in mem.chunks(8) {
            let id = u64::from_le_bytes(<[u8; 8]>::try_from(i).unwrap());
            out.push(match id {
                0 => WaitEntry::Empty,
                1 => WaitEntry::InterfaceOrProcDestroyed,
                _ => WaitEntry::Answer(MessageId::try_from(id).unwrap()),
            });
        }
        out
    };

    let out_pointer = u32::try_from(
        params[2]
            .into_i32()
            .ok_or(ExtrinsicNextNotificationErr::BadParameter)?,
    )
    .map_err(|_| ExtrinsicNextNotificationErr::BadParameter)?;
    let out_size = u32::try_from(
        params[3]
            .into_i32()
            .ok_or(ExtrinsicNextNotificationErr::BadParameter)?,
    )
    .map_err(|_| ExtrinsicNextNotificationErr::BadParameter)?;
    let flags = params[4]
        .into_i64()
        .ok_or(ExtrinsicNextNotificationErr::BadParameter)?;

    Ok(NotificationWait {
        notifs_ids,
        notifs_ids_ptr,
        out_pointer,
        out_size,
        block: (flags & 0x1) != 0,
    })
}

/// How a process is waiting for messages.
#[derive(Debug, PartialEq, Eq)]
pub struct NotificationWait {
    /// List of notifications the thread is waiting upon. Copy of what is in the process's memory.
    pub notifs_ids: Vec<WaitEntry>,
    /// Offset within the memory of the process where the list of notifications to wait upon is
    /// located. This is required to zero that location.
    pub notifs_ids_ptr: u32,
    /// Offset within the memory of the process where to write the received notification.
    pub out_pointer: u32,
    /// Size of the memory of the process dedicated to receiving the notification.
    pub out_size: u32,
    /// Whether to block the thread if no notification is available.
    pub block: bool,
}

/// What a thread is waiting upon.
// TODO: would be cool if this representation of that was just a u64
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitEntry {
    /// An empty entry. Serves no purpose but it might be convenient for the user of this call
    /// to leave entries empty.
    Empty,
    /// Waiting for either an interface notification or a process destroyed notification.
    InterfaceOrProcDestroyed,
    /// Waiting for an answer to the given message.
    Answer(MessageId),
}

/// Error that [`parse_extrinsic_next_notification`] can return.
#[derive(Debug)]
pub enum ExtrinsicNextNotificationErr {
    /// Too many notification ids requested.
    TooManyNotificationIds {
        /// Number of notification IDs that have been requested.
        requested: u32,
    },
    /// Bad type or invalid value for a parameter.
    BadParameter,
}

/// Analyzes a call to `emit_message` made by the given thread.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
pub fn parse_extrinsic_emit_message<TExtr, TPud, TTud>(
    thread: &mut processes::ThreadAccess<TExtr, TPud, TTud>,
    params: Vec<crate::WasmValue>,
) -> Result<EmitMessage, ExtrinsicEmitMessageErr> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 6);

    let interface: InterfaceHash = {
        let addr = u32::try_from(
            params[0]
                .into_i32()
                .ok_or(ExtrinsicEmitMessageErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?;
        InterfaceHash::from(
            <[u8; 32]>::try_from(
                &thread
                    .read_memory(addr, 32)
                    .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?[..],
            )
            .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?,
        )
    };

    let message = {
        let addr = u32::try_from(
            params[1]
                .into_i32()
                .ok_or(ExtrinsicEmitMessageErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?;
        let num_bufs = u32::try_from(
            params[2]
                .into_i32()
                .ok_or(ExtrinsicEmitMessageErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?;
        let mut out_msg = Vec::new();
        for buf_n in 0..num_bufs {
            let sub_buf_ptr = thread
                .read_memory(addr + 8 * buf_n, 4)
                .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?;
            let sub_buf_ptr = u32::from_le_bytes(<[u8; 4]>::try_from(&sub_buf_ptr[..]).unwrap());
            let sub_buf_sz = thread
                .read_memory(addr + 8 * buf_n + 4, 4)
                .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?;
            let sub_buf_sz = u32::from_le_bytes(<[u8; 4]>::try_from(&sub_buf_sz[..]).unwrap());
            if out_msg.len()
                + usize::try_from(sub_buf_sz).map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?
                >= 16 * 1024 * 1024
            {
                // TODO: arbitrary maximum message length
                panic!("Max message length reached");
                //return Err(());
            }
            out_msg.extend_from_slice(
                &thread
                    .read_memory(sub_buf_ptr, sub_buf_sz)
                    .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?,
            );
        }
        EncodedMessage(out_msg)
    };

    let needs_answer = params[3]
        .into_i32()
        .ok_or(ExtrinsicEmitMessageErr::BadParameter)?
        != 0;
    let allow_delay = params[4]
        .into_i32()
        .ok_or(ExtrinsicEmitMessageErr::BadParameter)?
        != 0;
    let message_id_write = if needs_answer {
        Some(
            u32::try_from(
                params[5]
                    .into_i32()
                    .ok_or(ExtrinsicEmitMessageErr::BadParameter)?,
            )
            .map_err(|_| ExtrinsicEmitMessageErr::BadParameter)?,
        )
    } else {
        None
    };

    Ok(EmitMessage {
        interface,
        message_id_write,
        message,
        allow_delay,
    })
}

/// How a process is emitting a message.
#[derive(Debug, PartialEq, Eq)]
pub struct EmitMessage {
    /// Interface the process wants to emit the message on.
    pub interface: InterfaceHash,
    /// Location in the process' memory where to write the generated message ID, or `None` if no
    /// answer is expected.
    pub message_id_write: Option<u32>,
    /// Message itself. Needs to be delivered to the interface handler.
    pub message: EncodedMessage,
    /// True if we're allowed to block the thread to wait for an interface handler to be
    /// available.
    pub allow_delay: bool,
}

/// Error that [`parse_extrinsic_emit_message`] can return.
#[derive(Debug)]
pub enum ExtrinsicEmitMessageErr {
    /// Bad type or invalid value for a parameter.
    BadParameter,
}

/// Analyzes a call to `emit_answer` made by the given thread.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
pub fn parse_extrinsic_emit_answer<TExtr, TPud, TTud>(
    thread: &mut processes::ThreadAccess<TExtr, TPud, TTud>,
    params: Vec<crate::WasmValue>,
) -> Result<EmitAnswer, ExtrinsicEmitAnswerErr> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 3);

    let message_id = {
        let addr = u32::try_from(
            params[0]
                .into_i32()
                .ok_or(ExtrinsicEmitAnswerErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitAnswerErr::BadParameter)?;
        let buf = thread
            .read_memory(addr, 8)
            .map_err(|_| ExtrinsicEmitAnswerErr::BadParameter)?;
        let id = u64::from_le_bytes(<[u8; 8]>::try_from(&buf[..]).unwrap());
        MessageId::try_from(id).map_err(ExtrinsicEmitAnswerErr::InvalidMessageId)?
    };

    let response = {
        let addr = u32::try_from(
            params[1]
                .into_i32()
                .ok_or(ExtrinsicEmitAnswerErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitAnswerErr::BadParameter)?;
        let sz = u32::try_from(
            params[2]
                .into_i32()
                .ok_or(ExtrinsicEmitAnswerErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitAnswerErr::BadParameter)?;
        EncodedMessage(
            thread
                .read_memory(addr, sz)
                .map_err(|_| ExtrinsicEmitAnswerErr::BadParameter)?,
        )
    };

    Ok(EmitAnswer {
        message_id,
        response,
    })
}

/// How a process is emitting a response.
#[derive(Debug, PartialEq, Eq)]
pub struct EmitAnswer {
    /// Identifier of the message to answer.
    pub message_id: MessageId,
    /// The response itself.
    pub response: EncodedMessage,
}

/// Error that [`parse_extrinsic_emit_answer`] can return.
#[derive(Debug)]
pub enum ExtrinsicEmitAnswerErr {
    /// Bad type or invalid value for a parameter.
    BadParameter,
    /// The message id is invalid.
    InvalidMessageId(InvalidMessageIdErr),
}

/// Analyzes a call to `emit_message_error` made by the given thread.
/// Returns the message for which to notify of an error.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
pub fn parse_extrinsic_emit_message_error<TExtr, TPud, TTud>(
    thread: &mut processes::ThreadAccess<TExtr, TPud, TTud>,
    params: Vec<crate::WasmValue>,
) -> Result<MessageId, ExtrinsicEmitMessageErrorErr> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 1);

    let msg_id = {
        let addr = u32::try_from(
            params[0]
                .into_i32()
                .ok_or(ExtrinsicEmitMessageErrorErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicEmitMessageErrorErr::BadParameter)?;
        let buf = thread
            .read_memory(addr, 8)
            .map_err(|_| ExtrinsicEmitMessageErrorErr::BadParameter)?;
        let id = u64::from_le_bytes(<[u8; 8]>::try_from(&buf[..]).unwrap());
        MessageId::try_from(id).map_err(ExtrinsicEmitMessageErrorErr::InvalidMessageId)?
    };

    Ok(msg_id)
}

/// Error that [`parse_extrinsic_emit_message_error`] can return.
#[derive(Debug)]
pub enum ExtrinsicEmitMessageErrorErr {
    /// Bad type or invalid value for a parameter.
    BadParameter,
    /// The message id is invalid.
    InvalidMessageId(InvalidMessageIdErr),
}

/// Analyzes a call to `cancel_message` made by the given thread.
/// Returns the message to cancel.
///
/// The `thread` parameter is only used in order to read memory from the process. This function
/// has no side effect.
///
/// Returns an error if the call is invalid.
pub fn parse_extrinsic_cancel_message<TExtr, TPud, TTud>(
    thread: &mut processes::ThreadAccess<TExtr, TPud, TTud>,
    params: Vec<crate::WasmValue>,
) -> Result<MessageId, ExtrinsicCancelMessageErr> {
    // We use an assert here rather than a runtime check because the WASM VM (rather than us) is
    // supposed to check the function signature.
    assert_eq!(params.len(), 1);

    let msg_id = {
        let addr = u32::try_from(
            params[0]
                .into_i32()
                .ok_or(ExtrinsicCancelMessageErr::BadParameter)?,
        )
        .map_err(|_| ExtrinsicCancelMessageErr::BadParameter)?;
        let buf = thread
            .read_memory(addr, 8)
            .map_err(|_| ExtrinsicCancelMessageErr::BadParameter)?;
        let id = u64::from_le_bytes(<[u8; 8]>::try_from(&buf[..]).unwrap());
        MessageId::try_from(id).map_err(ExtrinsicCancelMessageErr::InvalidMessageId)?
    };

    Ok(msg_id)
}

/// Error that [`parse_extrinsic_cancel_message`] can return.
#[derive(Debug)]
pub enum ExtrinsicCancelMessageErr {
    /// Bad type or invalid value for a parameter.
    BadParameter,
    /// The message id is invalid.
    InvalidMessageId(InvalidMessageIdErr),
}
