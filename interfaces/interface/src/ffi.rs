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

use alloc::vec::Vec;
use core::{convert::TryFrom as _, num::NonZeroU64};
use redshirt_syscalls::{EncodedMessage, InterfaceHash, MessageId, Pid};

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0x49, 0x6e, 0x56, 0x14, 0x8c, 0xd4, 0x2b, 0xc3, 0x9b, 0x4e, 0xbf, 0x5e, 0xb6, 0x2c, 0x60, 0x4d,
    0x7d, 0xd5, 0x70, 0x92, 0x4d, 0x4f, 0x70, 0xdf, 0xb3, 0xda, 0xf6, 0xfe, 0xdc, 0x65, 0x93, 0x8a,
]);

#[derive(Debug, parity_scale_codec::Encode, parity_scale_codec::Decode)]
pub enum InterfaceMessage {
    Register(InterfaceHash),
    NextMessage(NonZeroU64),
}

#[derive(Debug, parity_scale_codec::Encode, parity_scale_codec::Decode)]
pub struct InterfaceRegisterResponse {
    pub result: Result<NonZeroU64, InterfaceRegisterError>,
}

#[derive(Debug, Clone, parity_scale_codec::Encode, parity_scale_codec::Decode)]
pub enum InterfaceRegisterError {
    /// There already exists a process registered for this interface.
    AlreadyRegistered,
}

/// Either a decoded interface notification or a decoded process destroyed notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedInterfaceOrDestroyed {
    /// Interface notification.
    Interface(DecodedInterfaceNotification),
    /// Process destroyed notification.
    ProcessDestroyed(DecodedProcessDestroyedNotification),
}

/// Attempt to decode a notification.
pub fn decode_notification(buffer: &[u8]) -> Result<DecodedInterfaceOrDestroyed, ()> {
    if buffer.is_empty() {
        return Err(());
    }

    match buffer[0] {
        0 => decode_interface_notification(buffer).map(DecodedInterfaceOrDestroyed::Interface),
        2 => {
            decode_process_destroyed_notification(buffer).map(DecodedInterfaceOrDestroyed::ProcessDestroyed)
        }
        _ => Err(()),
    }
}

/// Builds a interface notification from its raw components.
pub fn build_interface_notification(
    interface: &InterfaceHash,
    message_id: Option<MessageId>,
    emitter_pid: Pid,
    actual_data: &EncodedMessage,
) -> InterfaceNotificationBuilder {
    let mut buffer = Vec::with_capacity(1 + 32 + 8 + 8 + 4 + actual_data.0.len());
    buffer.push(0);
    buffer.extend_from_slice(interface.as_ref());
    buffer.extend_from_slice(&message_id.map(u64::from).unwrap_or(0).to_le_bytes());
    buffer.extend_from_slice(&u64::from(emitter_pid).to_le_bytes());
    buffer.extend_from_slice(&0u32.to_le_bytes());  // TODO: remove field entirely
    buffer.extend_from_slice(&actual_data.0);

    debug_assert_eq!(buffer.capacity(), buffer.len());
    InterfaceNotificationBuilder { data: buffer }
}

#[derive(Debug, Clone)]
pub struct InterfaceNotificationBuilder {
    data: Vec<u8>,
}

impl InterfaceNotificationBuilder {
    /// Returns the [`MessageId`] that was put in the builder.
    pub fn message_id(&self) -> Option<MessageId> {
        let id = u64::from_le_bytes([
            self.data[33],
            self.data[34],
            self.data[35],
            self.data[36],
            self.data[37],
            self.data[38],
            self.data[39],
            self.data[40],
        ]);

        MessageId::try_from(id).ok()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn into_bytes(self) -> Vec<u8> {  // TODO: return EncodedMessage
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
        interface: InterfaceHash::from({
            let mut hash = [0; 32];
            hash.copy_from_slice(&buffer[1..33]);
            hash
        }),
        message_id: {
            let id = u64::from_le_bytes([
                buffer[33], buffer[34], buffer[35], buffer[36], buffer[37], buffer[38], buffer[39],
                buffer[40],
            ]);

            MessageId::try_from(id).ok()
        },
        emitter_pid: From::from(u64::from_le_bytes([
            buffer[41], buffer[42], buffer[43], buffer[44], buffer[45], buffer[46], buffer[47],
            buffer[48],
        ])),
        actual_data: EncodedMessage(buffer[53..].to_vec()),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedInterfaceNotification {
    /// Interface the message concerns.
    // TODO: remove
    pub interface: InterfaceHash,
    /// Id of the message. Can be used for answering. `None` if no answer is expected.
    pub message_id: Option<MessageId>,
    /// Id of the process that emitted the message.
    ///
    /// This should be used for security purposes, so that a process can't modify another process'
    /// resources.
    pub emitter_pid: Pid,
    pub actual_data: EncodedMessage,
}

pub fn build_process_destroyed_notification(
    pid: Pid,
) -> ProcessDestroyedNotificationBuilder {
    let mut buffer = Vec::with_capacity(1 + 8 + 4);
    buffer.push(2);
    buffer.extend_from_slice(&u64::from(pid).to_le_bytes());
    buffer.extend_from_slice(&0u32.to_le_bytes());  // TODO: remove field entirely

    debug_assert_eq!(buffer.capacity(), buffer.len());
    ProcessDestroyedNotificationBuilder { data: buffer }
}

#[derive(Debug, Clone)]
pub struct ProcessDestroyedNotificationBuilder {
    data: Vec<u8>,
}

impl ProcessDestroyedNotificationBuilder {
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
        ]))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedProcessDestroyedNotification {
    /// Identifier of the process that got destroyed.
    pub pid: Pid,
}
