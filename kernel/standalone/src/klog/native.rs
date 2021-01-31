// Copyright (C) 2019-2021  Pierre Krieger
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

//! Native program that handles the `kernel_log` interface.

use crate::arch::PlatformSpecific;

use alloc::sync::Arc;
use core::{pin::Pin, str};
use redshirt_core::EncodedMessage;

/// State machine for `kernel_log` interface messages handling.
pub struct KernelLogNativeProgram {
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<PlatformSpecific>>,
}

impl KernelLogNativeProgram {
    /// Initializes the native program.
    pub fn new(platform_specific: Pin<Arc<PlatformSpecific>>) -> Self {
        KernelLogNativeProgram { platform_specific }
    }

    pub fn interface_message(&self, message: &EncodedMessage) {
        match message.0.get(0) {
            Some(0) => {
                // Log message.
                let message = &message.0[1..];
                if message.is_ascii() {
                    self.platform_specific
                        .write_log(str::from_utf8(message).unwrap());
                }
            }
            Some(1) => {
                // New log method.
                unimplemented!(); // TODO:
                                  /*if let Ok(method) = KernelLogMethod::decode(&message.0[1..]) {
                                      self.klogger.set_method(method);
                                      if let Some(message_id) = notification.message_id {
                                          self.pending_messages_tx.unbounded_send((message_id, Ok(().encode())))
                                      }
                                  } else {
                                      if let Some(message_id) = notification.message_id {
                                          self.pending_messages_tx.unbounded_send((message_id, Err(())))
                                      }
                                  }*/
            }
            _ => {}
        }
    }
}
