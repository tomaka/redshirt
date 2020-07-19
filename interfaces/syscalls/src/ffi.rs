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

use crate::{EncodedMessage, MessageId};

use alloc::vec::Vec;
use core::convert::TryFrom as _;

#[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
#[link(wasm_import_module = "redshirt")]
extern "C" {
    /// Asks for the next notification.
    ///
    /// The `to_poll` parameter must be a list (whose length is `to_poll_len`) of messages whose
    /// answer to poll. Entries in this list equal to `0` are ignored. If a notification is
    /// successfully pulled, the corresponding entry in `to_poll` is set to `0`.
    ///
    /// Flags is a bitfield, defined as:
    ///
    /// - Bit 0: the `block` flag. If set, then this function puts the thread to sleep until a
    /// notification is available. Otherwise, this function returns as soon as possible.
    ///
    /// If the function returns 0, then there is no notification available and nothing has been
    /// written.
    /// This function never returns 0 if the `block` flag is set.
    /// If the function returns a value larger than `out_len`, then a notification is available
    /// whose  length is the value that has been returned, but nothing has been written in `out`.
    /// If the function returns value inferior or equal to `out_len` (and different from 0), then
    /// a notification has been written in `out`. `out` must be 8-bytes-aligned.
    ///
    /// Messages, amongst the set that matches `to_poll`, are always returned in the order they
    /// have been received. In particular, this function does **not** search the queue of
    /// notifications for a notification that fits in `out_len`. It will however skip the
    /// notifications in the queue that do not match any entry in `to_poll`.
    ///
    /// Messages written in `out` can be decoded using [`decode_notification`].
    ///
    /// When this function is being called, a "lock" is being held on the memory pointed by
    /// `to_poll` and `out`. In particular, it is invalid to modify these buffers while the
    /// function is running.
    pub(crate) fn next_notification(
        to_poll: *mut u64,
        to_poll_len: u32,
        out: *mut u8,
        out_len: u32,
        flags: u64,
    ) -> u32;

    /// Sends a message to the process that has registered the given interface.
    ///
    /// The memory area pointed to by `msg_bufs_ptrs` must contain a list of `msg_bufs_num` pairs
    /// of two 32-bits values encoded in little endian. In other words, the list must contain
    /// `msg_bufs_num * 2` values. Each pair is composed of a memory address and a length
    /// referring to a buffer containing a slice of the message body.
    /// The message body consists of the concatenation of all these buffers.
    ///
    /// > **Note**: This API is similar to the one of the `writev` POSIX function. The
    /// >           `msg_bufs_ptrs` parameter is similar to the `iov` parameter of `writev`, and
    /// >           the `msg_bufs_num` parameter is similar to the `iovcnt` parameter of `writev`.
    ///
    /// The message body is what will go into the
    /// [`actual_data`](DecodedInterfaceNotification::actual_data) field of the
    /// [`DecodedInterfaceNotification`] that the target will receive.
    ///
    /// Flags is a bitfield, defined as:
    ///
    /// - Bit 0: the `needs_answer` flag. If set, then this message expects an answer.
    /// - Bit 1: the `allow_delay` flag. If set, the kernel is allowed to block the thread in
    /// order to lazily-load a handler for that interface if necessary. If this flag is not set,
    /// and no interface handler is available, then the function fails immediately.
    ///
    /// Returns `0` on success, and `1` in case of error.
    ///
    /// On success, if `needs_answer` is true, will write the ID of new event into the memory
    /// pointed by `message_id_out`.
    ///
    /// When this function is being called, a "lock" is being held on the memory pointed by
    /// `interface_hash`, `msg_bufs_ptrs`, `message_id_out`, and all the sub-buffers referred to
    /// within `msg_bufs_ptrs`. In particular, it is invalid to modify these buffers while the
    /// function is running.
    // TODO: document error that can happen
    pub(crate) fn emit_message(
        interface_hash: *const u8,
        msg_bufs_ptrs: *const u32,
        msg_bufs_num: u32,
        flags: u64,
        message_id_out: *mut u64,
    ) -> u32;

    /// Sends an answer back to the emitter of given `message_id`.
    ///
    /// Has no effect if the message id is zero or refers to an invalid message. This can
    /// legitimately happen if the process that emitted the message has crashed or stopped.
    ///
    /// When this function is being called, a "lock" is being held on the memory pointed by
    /// `message_id` and `msg`. In particular, it is invalid to modify these buffers while the
    /// function is running.
    pub(crate) fn emit_answer(message_id: *const u64, msg: *const u8, msg_len: u32);

    /// Notifies the kernel that the given message is invalid and cannot reasonably be answered.
    ///
    /// Has no effect if the message id is zero or refers to an invalid message. This can
    /// legitimately happen if the process that emitted the message has crashed or stopped.
    ///
    /// This should be used in situations where a message we receive fails to parse or is generally
    /// invalid. In other words, this should only be used in case of misbehaviour by the sender.
    ///
    /// When this function is being called, a "lock" is being held on the memory pointed by
    /// `message_id`. In particular, it is invalid to modify these buffers while the function is
    /// running.
    pub(crate) fn emit_message_error(message_id: *const u64);

    /// Cancel an expected answer.
    ///
    /// After a message that needs an answer has been emitted using `emit_message`,
    /// the `cancel_message` function can be used to signal that we're not interested in the
    /// answer.
    ///
    /// After this function has been called, the passed `message_id` is no longer valid.
    /// Has no effect if the message id is zero or refers to an invalid message. This can
    /// legitimately happen if the process that emitted the message has crashed or stopped.
    ///
    /// When this function is being called, a "lock" is being held on the memory pointed by
    /// `message_id`. In particular, it is invalid to modify this buffer while the function is
    /// running.
    pub(crate) fn cancel_message(message_id: *const u64);
}

// TODO: all the decoding performs unaligned reads, which isn't great

pub fn build_notification(
    message_id: MessageId,
    index_in_list: u32,
    actual_data: Result<&EncodedMessage, ()>,
) -> NotificationBuilder {
    let mut buffer =
        Vec::with_capacity(1 + 8 + 4 + 1 + actual_data.map(|m| m.0.len()).unwrap_or(0));
    buffer.push(1);
    buffer.extend_from_slice(&u64::from(message_id).to_le_bytes());
    buffer.extend_from_slice(&index_in_list.to_le_bytes());
    if let Ok(actual_data) = actual_data {
        buffer.push(0);
        buffer.extend_from_slice(&actual_data.0);
    } else {
        buffer.push(1);
    }

    debug_assert_eq!(buffer.capacity(), buffer.len());
    NotificationBuilder { data: buffer }
}

#[derive(Debug, Clone)]
pub struct NotificationBuilder {
    data: Vec<u8>,
}

impl NotificationBuilder {
    /// Updates the `index_in_list` field of the message.
    pub fn set_index_in_list(&mut self, value: u32) {
        self.data[9..13].copy_from_slice(&value.to_le_bytes());
    }

    pub fn message_id(&self) -> MessageId {
        MessageId::try_from(u64::from_le_bytes([
            self.data[1],
            self.data[2],
            self.data[3],
            self.data[4],
            self.data[5],
            self.data[6],
            self.data[7],
            self.data[8],
        ]))
        .unwrap()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}

pub fn decode_notification(buffer: &[u8]) -> Result<DecodedNotification, ()> {
    if buffer.len() < 1 + 8 + 4 + 1 {
        return Err(());
    }

    if buffer[0] != 0x1 {
        return Err(());
    }

    let success = buffer[13] == 0;
    if !success && buffer.len() != 1 + 8 + 4 + 1 {
        return Err(());
    }

    Ok(DecodedNotification {
        message_id: MessageId::try_from({
            u64::from_le_bytes([
                buffer[1], buffer[2], buffer[3], buffer[4], buffer[5], buffer[6], buffer[7],
                buffer[8],
            ])
        })
        .map_err(|_| ())?,
        index_in_list: u32::from_le_bytes([buffer[9], buffer[10], buffer[11], buffer[12]]),
        actual_data: if success {
            Ok(EncodedMessage(buffer[14..].to_vec()))
        } else {
            Err(())
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedNotification {
    /// Identifier of the message whose answer we are receiving.
    pub message_id: MessageId,

    /// Index within the list to poll where this message was.
    pub index_in_list: u32,

    /// The response, or `Err` if:
    ///
    /// - The interface handler has crashed.
    /// - The interface handler marked our message as invalid.
    ///
    pub actual_data: Result<EncodedMessage, ()>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use core::{convert::TryFrom, num::NonZeroU64};

    #[test]
    fn response_message_encode_decode() {
        let message_id = TryFrom::try_from(0x0123456789abcdef).unwrap();
        let index_in_list = 0xdeadbeef;
        let message = EncodedMessage(vec![8, 7, 9]);

        let mut resp_notif = build_notification(message_id, 0xf00baa, Ok(&message));
        resp_notif.set_index_in_list(index_in_list);
        assert_eq!(resp_notif.message_id(), message_id);

        let decoded = decode_notification(&resp_notif.into_bytes()).unwrap();
        assert_eq!(decoded.message_id, message_id);
        assert_eq!(decoded.index_in_list, index_in_list);
        assert_eq!(decoded.actual_data, Ok(message));
    }
}
