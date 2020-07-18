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

use crate::{Encode, MessageId};

/// Answers the given message.
// TODO: move to interface interface?
pub fn emit_answer(message_id: MessageId, msg: impl Encode) {
    #[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
    fn imp(message_id: MessageId, msg: impl Encode) {
        unsafe {
            let buf = msg.encode();
            crate::ffi::emit_answer(&u64::from(message_id), buf.0.as_ptr(), buf.0.len() as u32);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn imp(message_id: MessageId, msg: impl Encode) {
        unreachable!()
    }
    imp(message_id, msg)
}

/// Answers the given message by notifying of an error in the message.
// TODO: move to interface interface?
pub fn emit_message_error(message_id: MessageId) {
    #[cfg(target_arch = "wasm32")] // TODO: we should have a proper operating system name instead
    fn imp(message_id: MessageId) {
        unsafe { crate::ffi::emit_message_error(&u64::from(message_id)) }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn imp(message_id: MessageId) {
        unreachable!()
    }
    imp(message_id)
}
