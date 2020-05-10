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

//! Endpoint List management.
//!
//! One of the most important part of the OHCI specs is the "endpoint lists processing". The host
//! must maintain a certain number of **endpoint lists** in memory that the USB controller will
//! read and process. This module represents one such lists.
//!
//! Each endpoint list is a linked list of **endpoint descriptors**. Each endpoint descriptor
//! is specific to one USB endpoint. A USB endpoint is a functionality on a USB device.
//!
//! Each endpoint descriptor contains, in turn, a linked list of **transfer descriptors** (TD).
//! Each transfer descriptor represents one transfer to be performed from or to the USB endpoint
//! referred to by the endpoint descriptor.

use crate::{ohci::ep_descriptor, HwAccessRef};

use alloc::vec::Vec;
use core::num::NonZeroU32;

pub use ep_descriptor::{Config, Direction};

/// Linked list of endpoint descriptors.
pub struct EndpointList<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Hardware abstraction layer.
    hardware_access: TAcc,
    /// The list always starts with a dummy descriptor, allowing us to have a constant start.
    /// This not something enforced by the specs, but it is recommended by the specs for ease of
    /// implementation.
    dummy_descriptor: ep_descriptor::EndpointDescriptor<TAcc>,
    /// List of descriptors linked to each other.
    descriptors: Vec<ep_descriptor::EndpointDescriptor<TAcc>>,
}

impl<TAcc> EndpointList<TAcc>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Initializes a new endpoint descriptors list.
    pub async fn new(hardware_access: TAcc) -> EndpointList<TAcc> {
        let dummy_descriptor = {
            let config = Config {
                maximum_packet_size: 0,
                function_address: 0,
                endpoint_number: 0,
                isochronous: false, // TODO: must be correct I guess
                low_speed: false,
                direction: Direction::FromTd,
            };

            ep_descriptor::EndpointDescriptor::new(hardware_access.clone(), config).await
        };

        EndpointList {
            hardware_access,
            dummy_descriptor,
            descriptors: Vec::new(),
            next_transfer_descriptor,
        }
    }

    /// Sets the next endpoint list in the linked list.
    ///
    /// Endpoint lists are always part of a linked list, where each list points to the
    /// next one, or to nothing.
    ///
    /// # Safety
    ///
    /// `next` must remain valid until the next time [`EndpointList::clear_next`] or
    /// [`EndpointDescriptor::EndpointList`] is called, or until this [`EndpointList`] is
    /// destroyed.
    pub async unsafe fn set_next<UAcc>(&mut self, next: &EndpointList<UAcc>)
    where
        UAcc: Clone,
        for<'r> &'r UAcc: HwAccessRef<'r>,
    {
        let current_last = self
            .descriptors
            .last_mut()
            .unwrap_or(&mut self.dummy_descriptor);
        current_last.set_next_raw(next.head_pointer().get()).await;
    }

    /// Returns the physical memory address of the head of the list.
    ///
    /// This value never changes and is valid until the [`EndpointList`] is destroyed.
    pub fn head_pointer(&self) -> NonZeroU32 {
        self.dummy_descriptor.pointer()
    }

    /// Adds a new endpoint descriptor to the list.
    pub async fn push(&mut self, config: Config) {
        let current_last = self
            .descriptors
            .last_mut()
            .unwrap_or(&mut self.dummy_descriptor);

        let mut new_descriptor =
            ep_descriptor::EndpointDescriptor::new(self.hardware_access.clone(), config).await;

        unsafe {
            // The order here is important. First make the new descriptor pointer to the current
            // location, then only pointer to that new descriptor. This ensures that the
            // controller doesn't jump to the new descriptor before it's ready.
            new_descriptor
                .set_next_raw(current_last.get_next_raw().await)
                .await;
            current_last.set_next(&new_descriptor).await;
        }

        self.descriptors.push(new_descriptor);
    }
}
