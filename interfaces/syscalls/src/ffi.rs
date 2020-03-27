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

use crate::{EncodedMessage, InterfaceHash, MessageId, Pid};

use alloc::vec::Vec;

#[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
#[link(wasm_import_module = "redshirt")]
extern "C" {
    /// Asks for the next notification.
    ///
    /// The `to_poll` parameter must be a list (whose length is `to_poll_len`) of notifications to
    /// poll. Entries in this list equal to `0` are ignored. Entries equal to `1` are special and
    /// mean "a message received on an interface or a process destroyed notification". If a
    /// notification is successfully pulled, the corresponding entry in `to_poll` is set to `0`.
    ///
    /// If `block` is true, then this function puts the thread to sleep until a notification is
    /// available. If `block` is false, then this function returns as soon as possible.
    ///
    /// If the function returns 0, then there is no notification available and nothing has been
    /// written.
    /// This function never returns 0 if `block` is `true`.
    /// If the function returns a value larger than `out_len`, then a notification is available
    /// whose  length is the value that has been returned, but nothing has been written in `out`.
    /// If the function returns value inferior or equal to `out_len` (and different from 0), then
    /// a notification has been written in `out`.
    ///
    /// Messages, amongst the set that matches `to_poll`, are always returned in the order they
    /// have been received. In particular, this function does **not** search the queue of
    /// notifications for a notification that fits in `out_len`. It will however skip the
    /// notifications in the queue that do not match any entry in `to_poll`.
    ///
    /// Messages written in `out` can be decoded into a [`DecodedNotification`].
    ///
    /// When this function is being called, a "lock" is being held on the memory pointed by
    /// `to_poll` and `out`. In particular, it is invalid to modify these buffers while the
    /// function is running.
    pub(crate) fn next_notification(
        to_poll: *mut u64,
        to_poll_len: u32,
        out: *mut u8,
        out_len: u32,
        block: bool,
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
    /// Returns `0` on success, and `1` in case of error.
    ///
    /// On success, if `needs_answer` is true, will write the ID of new event into the memory
    /// pointed by `message_id_out`.
    ///
    /// If `allow_delay` is true, the kernel is allowed to block the thread in order to
    /// lazily-load a handler for that interface if necessary. If `allow_delay` is false and no
    /// interface handler is available, the function fails immediately.
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
        needs_answer: bool,
        allow_delay: bool,
        message_id_out: *mut u64,
    ) -> u32;

    /// Sends an answer back to the emitter of given `message_id`.
    ///
    /// When this function is being called, a "lock" is being held on the memory pointed by
    /// `message_id` and `msg`. In particular, it is invalid to modify these buffers while the
    /// function is running.
    pub(crate) fn emit_answer(message_id: *const u64, msg: *const u8, msg_len: u32);

    /// Notifies the kernel that the given message is invalid and cannot reasonably be answered.
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
    ///
    /// When this function is being called, a "lock" is being held on the memory pointed by
    /// `message_id`. In particular, it is invalid to modify this buffer while the function is
    /// running.
    pub(crate) fn cancel_message(message_id: *const u64);
}

/// Prototype for a message.
#[derive(Debug, Clone)]
pub enum NotificationBuilder {
    /// Prototype for an interface message.
    Interface(InterfaceNotificationBuilder),
    /// Prototype for a response message.
    Response(ResponseNotificationBuilder),
    /// Prototype for a process destroyed message.
    ProcessDestroyed(ProcessDestroyedNotificationBuilder),
}

impl NotificationBuilder {
    /// Returns the length in bytes of the constructed message.
    pub fn len(&self) -> usize {
        match self {
            NotificationBuilder::Interface(msg) => msg.len(),
            NotificationBuilder::Response(msg) => msg.len(),
            NotificationBuilder::ProcessDestroyed(msg) => msg.len(),
        }
    }

    // TODO: change to a more strongly typed API
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            NotificationBuilder::Interface(msg) => msg.into_bytes(),
            NotificationBuilder::Response(msg) => msg.into_bytes(),
            NotificationBuilder::ProcessDestroyed(msg) => msg.into_bytes(),
        }
    }

    /// Modifies the `index_in_list` field of the message in construction.
    pub fn set_index_in_list(&mut self, value: u32) {
        match self {
            NotificationBuilder::Interface(msg) => msg.set_index_in_list(value),
            NotificationBuilder::Response(msg) => msg.set_index_in_list(value),
            NotificationBuilder::ProcessDestroyed(msg) => msg.set_index_in_list(value),
        }
    }
}

impl From<InterfaceNotificationBuilder> for NotificationBuilder {
    fn from(msg: InterfaceNotificationBuilder) -> NotificationBuilder {
        NotificationBuilder::Interface(msg)
    }
}

impl From<ResponseNotificationBuilder> for NotificationBuilder {
    fn from(msg: ResponseNotificationBuilder) -> NotificationBuilder {
        NotificationBuilder::Response(msg)
    }
}

impl From<ProcessDestroyedNotificationBuilder> for NotificationBuilder {
    fn from(msg: ProcessDestroyedNotificationBuilder) -> NotificationBuilder {
        NotificationBuilder::ProcessDestroyed(msg)
    }
}

/// Message received from the kernel.
#[derive(Debug, Clone)]
pub enum DecodedNotification {
    /// Interface notification.
    Interface(DecodedInterfaceNotification),
    /// Response notification.
    Response(DecodedResponseNotification),
    /// Process destroyed notification.
    ///
    /// Whenever a process that has emitted events on one of our interfaces stops, a
    /// `ProcessDestroyed` notification is sent.
    ProcessDestroyed(DecodedProcessDestroyedNotification),
}

// TODO: all the decoding performs unaligned reads, which isn't great

/// Attempt to decode a notification.
pub fn decode_notification(buffer: &[u8]) -> Result<DecodedNotification, ()> {
    if buffer.is_empty() {
        return Err(());
    }

    match buffer[0] {
        0 => decode_interface_notification(buffer).map(DecodedNotification::Interface),
        1 => decode_response_notification(buffer).map(DecodedNotification::Response),
        2 => {
            decode_process_destroyed_notification(buffer).map(DecodedNotification::ProcessDestroyed)
        }
        _ => Err(()),
    }
}

/// Either a decoded interface notification or a decoded process destroyed notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedInterfaceOrDestroyed {
    /// Interface notification.
    Interface(DecodedInterfaceNotification),
    /// Process destroyed notification.
    ProcessDestroyed(DecodedProcessDestroyedNotification),
}

/// Builds a interface notification from its raw components.
pub fn build_interface_notification(
    interface: &InterfaceHash,
    message_id: Option<MessageId>,
    emitter_pid: Pid,
    index_in_list: u32,
    actual_data: &EncodedMessage,
) -> InterfaceNotificationBuilder {
    let mut buffer = Vec::with_capacity(1 + 32 + 8 + 8 + 4 + actual_data.0.len());
    buffer.push(0);
    buffer.extend_from_slice(&interface.0);
    buffer.extend_from_slice(&message_id.map(u64::from).unwrap_or(0).to_le_bytes());
    buffer.extend_from_slice(&u64::from(emitter_pid).to_le_bytes());
    buffer.extend_from_slice(&index_in_list.to_le_bytes());
    buffer.extend_from_slice(&actual_data.0);

    debug_assert_eq!(buffer.capacity(), buffer.len());
    InterfaceNotificationBuilder { data: buffer }
}

#[derive(Debug, Clone)]
pub struct InterfaceNotificationBuilder {
    data: Vec<u8>,
}

impl InterfaceNotificationBuilder {
    /// Updates the `index_in_list` field of the message.
    pub fn set_index_in_list(&mut self, value: u32) {
        self.data[49..53].copy_from_slice(&value.to_le_bytes());
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}

pub fn decode_interface_notification(buffer: &[u8]) -> Result<DecodedInterfaceNotification, ()> {
    if buffer.len() < 1 + 32 + 8 + 8 + 4 {
        return Err(());
    }

    if buffer[0] != 0x0 {
        return Err(());
    }

    Ok(DecodedInterfaceNotification {
        interface: InterfaceHash({
            let mut hash = [0; 32];
            hash.copy_from_slice(&buffer[1..33]);
            hash
        }),
        message_id: {
            let id = u64::from_le_bytes([
                buffer[33], buffer[34], buffer[35], buffer[36], buffer[37], buffer[38], buffer[39],
                buffer[40],
            ]);

            if id == 0 {
                None
            } else {
                Some(From::from(id))
            }
        },
        emitter_pid: From::from(u64::from_le_bytes([
            buffer[41], buffer[42], buffer[43], buffer[44], buffer[45], buffer[46], buffer[47],
            buffer[48],
        ])),
        index_in_list: u32::from_le_bytes([buffer[49], buffer[50], buffer[51], buffer[52]]),
        actual_data: EncodedMessage(buffer[53..].to_vec()),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedInterfaceNotification {
    /// Interface the message concerns.
    pub interface: InterfaceHash,
    /// Id of the message. Can be used for answering. `None` if no answer is expected.
    pub message_id: Option<MessageId>,
    /// Id of the process that emitted the message.
    ///
    /// This should be used for security purposes, so that a process can't modify another process'
    /// resources.
    pub emitter_pid: Pid,
    /// Index within the list to poll where this message was.
    pub index_in_list: u32,
    pub actual_data: EncodedMessage,
}

pub fn build_response_notification(
    message_id: MessageId,
    index_in_list: u32,
    actual_data: Result<&EncodedMessage, ()>,
) -> ResponseNotificationBuilder {
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
    ResponseNotificationBuilder { data: buffer }
}

#[derive(Debug, Clone)]
pub struct ResponseNotificationBuilder {
    data: Vec<u8>,
}

impl ResponseNotificationBuilder {
    /// Updates the `index_in_list` field of the message.
    pub fn set_index_in_list(&mut self, value: u32) {
        self.data[9..13].copy_from_slice(&value.to_le_bytes());
    }

    pub fn message_id(&self) -> MessageId {
        From::from(u64::from_le_bytes([
            self.data[1],
            self.data[2],
            self.data[3],
            self.data[4],
            self.data[5],
            self.data[6],
            self.data[7],
            self.data[8],
        ]))
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}

pub fn decode_response_notification(buffer: &[u8]) -> Result<DecodedResponseNotification, ()> {
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

    Ok(DecodedResponseNotification {
        message_id: From::from(u64::from_le_bytes([
            buffer[1], buffer[2], buffer[3], buffer[4], buffer[5], buffer[6], buffer[7], buffer[8],
        ])),
        index_in_list: u32::from_le_bytes([buffer[9], buffer[10], buffer[11], buffer[12]]),
        actual_data: if success {
            Ok(EncodedMessage(buffer[14..].to_vec()))
        } else {
            Err(())
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedResponseNotification {
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

pub fn build_process_destroyed_notification(
    pid: Pid,
    index_in_list: u32,
) -> ProcessDestroyedNotificationBuilder {
    let mut buffer = Vec::with_capacity(1 + 8 + 4);
    buffer.push(2);
    buffer.extend_from_slice(&u64::from(pid).to_le_bytes());
    buffer.extend_from_slice(&index_in_list.to_le_bytes());

    debug_assert_eq!(buffer.capacity(), buffer.len());
    ProcessDestroyedNotificationBuilder { data: buffer }
}

#[derive(Debug, Clone)]
pub struct ProcessDestroyedNotificationBuilder {
    data: Vec<u8>,
}

impl ProcessDestroyedNotificationBuilder {
    /// Updates the `index_in_list` field of the message.
    pub fn set_index_in_list(&mut self, value: u32) {
        self.data[9..13].copy_from_slice(&value.to_le_bytes());
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}

pub fn decode_process_destroyed_notification(
    buffer: &[u8],
) -> Result<DecodedProcessDestroyedNotification, ()> {
    if buffer.len() != 1 + 8 + 4 {
        return Err(());
    }

    if buffer[0] != 0x2 {
        return Err(());
    }

    Ok(DecodedProcessDestroyedNotification {
        pid: From::from(u64::from_le_bytes([
            buffer[1], buffer[2], buffer[3], buffer[4], buffer[5], buffer[6], buffer[7], buffer[8],
        ])),
        index_in_list: u32::from_le_bytes([buffer[9], buffer[10], buffer[11], buffer[12]]),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedProcessDestroyedNotification {
    /// Identifier of the process that got destroyed.
    pub pid: Pid,
    /// Index within the list to poll where this message was.
    pub index_in_list: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn interface_message_encode_decode() {
        let interface_hash = From::from([0xca; 32]);
        let message_id = Some(From::from(0x0123456789abcdef));
        let pid = From::from(0xfedcba9876543210);
        let index_in_list = 0xdeadbeef;
        let message = EncodedMessage(vec![8, 7, 9]);

        let mut int_notif =
            build_interface_notification(&interface_hash, message_id, pid, 0xf00baa, &message);
        int_notif.set_index_in_list(index_in_list);

        let decoded = decode_interface_notification(&int_notif.into_bytes()).unwrap();
        assert_eq!(decoded.interface, interface_hash);
        assert_eq!(decoded.message_id, message_id);
        assert_eq!(decoded.emitter_pid, pid);
        assert_eq!(decoded.index_in_list, index_in_list);
        assert_eq!(decoded.actual_data, message);
    }

    #[test]
    fn response_message_encode_decode() {
        let message_id = From::from(0x0123456789abcdef);
        let index_in_list = 0xdeadbeef;
        let message = EncodedMessage(vec![8, 7, 9]);

        let mut resp_notif = build_response_notification(message_id, 0xf00baa, Ok(&message));
        resp_notif.set_index_in_list(index_in_list);
        assert_eq!(resp_notif.message_id(), message_id);

        let decoded = decode_response_notification(&resp_notif.into_bytes()).unwrap();
        assert_eq!(decoded.message_id, message_id);
        assert_eq!(decoded.index_in_list, index_in_list);
        assert_eq!(decoded.actual_data, Ok(message));
    }

    #[test]
    fn response_message_err_encode_decode() {
        let message_id = From::from(0xa123456789abcdef);
        let index_in_list = 0xdeadbeef;

        let mut resp_notif = build_response_notification(message_id, 0xf00baa, Err(()));
        resp_notif.set_index_in_list(index_in_list);
        assert_eq!(resp_notif.message_id(), message_id);

        let decoded = decode_response_notification(&resp_notif.into_bytes()).unwrap();
        assert_eq!(decoded.message_id, message_id);
        assert_eq!(decoded.index_in_list, index_in_list);
        assert_eq!(decoded.actual_data, Err(()));
    }

    #[test]
    fn process_destroyed_message_encode_decode() {
        let pid = From::from(0xfedcba9876543210);
        let index_in_list = 0xdeadbeef;

        let mut destr_notif = build_process_destroyed_notification(pid, 0xf00baa);
        destr_notif.set_index_in_list(index_in_list);

        let decoded = decode_process_destroyed_notification(&destr_notif.into_bytes()).unwrap();
        assert_eq!(decoded.pid, pid);
        assert_eq!(decoded.index_in_list, index_in_list);
    }
}
