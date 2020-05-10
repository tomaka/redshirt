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
//! One of the most important part of the OHCI specs is the "endpoint lists processing". This
//! library maintains a certain number of **endpoint lists** in memory that the USB controller
//! will read and process.
//!
//! Each endpoint list is a linked list of **endpoint descriptors**. Each endpoint descriptor
//! is specific to one USB endpoint. A USB endpoint is a functionality on a USB device.
//!
//! Each endpoint descriptor contains, in turn, a linked list of **transfer descriptors** (TD).
//! Each transfer descriptor represents one transfer to be performed from or to a USB device.

use crate::{HwAccessRef, ohci::ep_descriptor};

use alloc::vec::Vec;

/// Linked list of endpoint descriptors.
pub struct EndpointList<'a, TAcc: HwAccessRef<'a>> {
    /// List of descriptors linked to each other.
    descriptors: Vec<ep_descriptor::EndpointDescriptor<'a, TAcc>>,
}

impl<'a, TAcc: HwAccessRef<'a>> EndpointList<'a, TAcc> {
    pub async fn new() -> EndpointList<'a, TAcc> {
        EndpointList {
            descriptors: Vec::new(),
        }
    }
}
