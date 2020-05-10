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
//! read and process.
//!
//! Each endpoint list is a linked list of **endpoint descriptors**. Each endpoint descriptor
//! is specific to one USB endpoint. A USB endpoint is a functionality on a USB device.
//!
//! Each endpoint descriptor contains, in turn, a linked list of **transfer descriptors** (TD).
//! Each transfer descriptor represents one transfer to be performed from or to a USB device.

use crate::{ohci::ep_descriptor, HwAccessRef};

use alloc::vec::Vec;

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
    pub async fn new(hardware_access: TAcc) -> EndpointList<TAcc> {
        let dummy_descriptor = {
            let config = Config {
                maximum_packet_size: 0,
                function_address: 0,
                endpoint_number: 0,
                isochronous: false,  // TODO: must be correct I guess
                low_speed: false,
                direction: Direction::FromTd,
            };

            ep_descriptor::EndpointDescriptor::new(hardware_access.clone(), config).await
        };

        EndpointList {
            hardware_access,
            dummy_descriptor,
            descriptors: Vec::new(),
        }
    }

    pub async fn push(&mut self, config: Config) {
        let new_descriptor =
            ep_descriptor::EndpointDescriptor::new(self.hardware_access.clone(), config).await;
        self.descriptors.push(new_descriptor);
    }
}
