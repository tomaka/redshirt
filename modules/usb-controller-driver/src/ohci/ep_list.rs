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

pub use ep_descriptor::{CompletedTransferDescriptor, Config, Direction, TransferDescriptorConfig};

/// Linked list of endpoint descriptors.
pub struct EndpointList<TAcc, TEpUd>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Hardware abstraction layer.
    hardware_access: TAcc,
    /// True if this is a list of isochronous transfers.
    isochronous: bool,
    /// The list always starts with a dummy descriptor, allowing us to have a constant start.
    /// This not something enforced by the specs, but it is recommended by the specs for ease of
    /// implementation.
    dummy_descriptor: ep_descriptor::EndpointDescriptor<TAcc>,
    /// List of descriptors linked to each other.
    descriptors: Vec<(ep_descriptor::EndpointDescriptor<TAcc>, TEpUd)>,
}

impl<TAcc, TEpUd> EndpointList<TAcc, TEpUd>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Initializes a new endpoint descriptors list.
    pub async fn new(hardware_access: TAcc, isochronous: bool) -> EndpointList<TAcc, TEpUd> {
        let dummy_descriptor = {
            // Pass a dummy configuration. None of these fields matter.
            let config = Config {
                maximum_packet_size: 0,
                function_address: 0,
                endpoint_number: 0,
                isochronous,
                low_speed: false,
                direction: Direction::FromTd,
            };

            let mut d =
                ep_descriptor::EndpointDescriptor::new(hardware_access.clone(), config).await;
            d.set_skip(true).await;
            d
        };

        EndpointList {
            hardware_access,
            isochronous,
            dummy_descriptor,
            descriptors: Vec::new(),
        }
    }

    /// Sets the next endpoint list in the linked list.
    ///
    /// Endpoint lists are always part of a linked list, where each list points to the
    /// next one, or to nothing.
    ///
    /// > **Note**: There is no method to set the next list to nothing, as this isn't useful in
    /// >           practice.
    ///
    /// # Safety
    ///
    /// `next` must remain valid until the next time [`EndpointList::set_next`] or is called, or
    /// until this [`EndpointList`] is destroyed.
    pub async unsafe fn set_next<UAcc, UEpUd>(&mut self, next: &EndpointList<UAcc, UEpUd>)
    where
        UAcc: Clone,
        for<'r> &'r UAcc: HwAccessRef<'r>,
    {
        let current_last = self
            .descriptors
            .last_mut()
            .map(|v| &mut v.0)
            .unwrap_or(&mut self.dummy_descriptor);
        current_last.set_next_raw(next.head_pointer().get()).await;
    }

    /// Returns the physical memory address of the head of the list.
    ///
    /// This value never changes and is valid until the [`EndpointList`] is destroyed.
    pub fn head_pointer(&self) -> NonZeroU32 {
        self.dummy_descriptor.pointer()
    }

    /// Finish destroying the endpoints that have been scheduled for removal, and returns the
    /// transfer descriptors that haven't been processed.
    ///
    /// # Safety
    ///
    /// You must only call this function when the frame number is different from what it was when
    /// you last called [`Endpoint::remove`]. This guarantees that the controller is no longer
    /// accessing the endpoint descriptors that this function finishes destroying.
    ///
    /// The user data must match the one that was used when pushing descriptors.
    pub async unsafe fn finish_removal<TUd>(&mut self) -> Vec<CompletedTransferDescriptor<TUd>> {
        // TODO:
        unimplemented!()
    }

    /// Adds a new endpoint descriptor to the list.
    ///
    /// # Panic
    ///
    /// Panics if `config.isochronous` is not the same value as what was passed to `new`.
    pub async fn push(&mut self, config: Config, user_data: TEpUd) {
        assert_eq!(config.isochronous, self.isochronous);

        let current_last = self
            .descriptors
            .last_mut()
            .map(|v| &mut v.0)
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

        self.descriptors.push((new_descriptor, user_data));
    }

    /// Returns the first endpoint in the list whose user data matches the given condition.
    pub fn find_by_user_data(
        &mut self,
        mut condition: impl FnMut(&TEpUd) -> bool,
    ) -> Option<Endpoint<TAcc, TEpUd>> {
        let index = self
            .descriptors
            .iter()
            .position(move |(_, ud)| condition(ud))?;
        Some(Endpoint { list: self, index })
    }
}

/// Access to a single endpoint.
pub struct Endpoint<'a, TAcc, TEpUd>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    list: &'a mut EndpointList<TAcc, TEpUd>,
    index: usize,
}

impl<'a, TAcc, TEpUd> Endpoint<'a, TAcc, TEpUd>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Pushes a new packet at the end of the list of transfer descriptors.
    ///
    /// After this packet has been processed by the controller, it will be moved to the "done
    /// queue" of the HCCA where you will be able to figure out whether the transfer worked.
    pub async fn push_packet<'b, TUd: 'static>(
        &mut self,
        cfg: TransferDescriptorConfig<'b>,
        user_data: TUd,
    ) {
        self.list.descriptors[self.index]
            .0
            .push_packet(cfg, user_data)
            .await
    }

    /// Removes the endpoint from the list.
    pub async fn remove(self) {
        // TODO:
        unimplemented!()
    }
}
