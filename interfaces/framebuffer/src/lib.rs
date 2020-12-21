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

//! Framebuffer interface.
//!
//! Allows drawing an image.
//!
//! > **Note**: The fate of this interface is kind of vague. It is also unclear whether
//! >           keyboard/mouse input should be handled here as well. Use at your own risks.

#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use core::convert::TryFrom as _;
use redshirt_syscalls::{InterfaceHash, MessageId};

pub mod ffi;

/// Framebuffer containing pixel data.
pub struct Framebuffer {
    /// Identifier of the framebuffer. Used for all communications.
    id: u32,

    /// Interface used for this framebuffer.
    interface: &'static InterfaceHash,

    /// Width of the framebuffer in pixels.
    width: u32,
    /// Height of the framebuffer in pixels.
    height: u32,

    /// List of active messages that will be responded with incoming events.
    ///
    /// The capacity of this container also corresponds to the number of elements that we want to
    /// have in it at any given moment. In other words, there is no field in this struct indicating
    /// the number of events because that'd be redundant with `event_messages.capacity()`.
    event_messages: VecDeque<MessageId>,
}

impl Framebuffer {
    /// Initializes a new framebuffer of the given width and height.
    pub async fn new(with_events: bool, width: u32, height: u32) -> Self {
        let id = unsafe {
            let mut out = [0; 4];
            redshirt_random_interface::generate_in(&mut out).await;
            u32::from_le_bytes(out)
        };

        let interface = if with_events {
            &ffi::INTERFACE_WITH_EVENTS
        } else {
            &ffi::INTERFACE_WITHOUT_EVENTS
        };

        unsafe {
            let id_le_bytes = id.to_le_bytes();
            let width_le_bytes = width.to_le_bytes();
            let height_le_bytes = height.to_le_bytes();
            redshirt_syscalls::MessageBuilder::new()
                .add_data_raw(&[0])
                .add_data_raw(&id_le_bytes[..])
                .add_data_raw(&width_le_bytes[..])
                .add_data_raw(&height_le_bytes[..])
                .emit_without_response(interface)
                .unwrap();
        }

        let num_events_queue = if with_events { 10 } else { 0 };

        let mut fb = Framebuffer {
            id,
            interface,
            width,
            height,
            event_messages: VecDeque::with_capacity(num_events_queue),
        };
        fb.fill_event_messages();
        fb
    }

    /// Sets the data in the framebuffer.
    ///
    /// The size of `data` must be `width * height * 3`.
    pub fn set_data(&self, data: &[u8]) {
        unsafe {
            assert_eq!(
                data.len(),
                usize::try_from(
                    self.width
                        .checked_mul(self.height)
                        .unwrap()
                        .checked_mul(3)
                        .unwrap()
                )
                .unwrap()
            );

            let id_le_bytes = self.id.to_le_bytes();
            redshirt_syscalls::MessageBuilder::new()
                .add_data_raw(&[2])
                .add_data_raw(&id_le_bytes[..])
                .add_data_raw(data)
                .emit_without_response(self.interface)
                .unwrap();
        }
    }

    /// Returns the next event that the framebuffer receives.
    pub async fn next_event(&mut self) -> ffi::Event {
        if let Some(first_event) = self.event_messages.front() {
            let event = redshirt_syscalls::message_response(*first_event).await;
            self.event_messages.pop_front();
            self.fill_event_messages();
            event
        } else {
            futures::future::pending().await
        }
    }

    /// Pushes back events to `event_messages` until we reach the maximum.
    fn fill_event_messages(&mut self) {
        while self.event_messages.len() < self.event_messages.capacity() {
            let new_event = unsafe {
                redshirt_syscalls::MessageBuilder::new()
                    .add_data_raw(&[3])
                    .add_data_raw(&self.id.to_le_bytes()[..])
                    .emit_with_response_raw(self.interface)
                    .unwrap()
            };

            self.event_messages.push_back(new_event);
        }
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        unsafe {
            let id_le_bytes = self.id.to_le_bytes();
            redshirt_syscalls::MessageBuilder::new()
                .add_data_raw(&[1])
                .add_data_raw(&id_le_bytes[..])
                .emit_without_response(self.interface)
                .unwrap();
        }
    }
}
