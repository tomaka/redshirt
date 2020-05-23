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

//! Host Controller Communications Area (HCCA) management.
//!
//! See section 4.4 of the specs.
//!
//! The HCCA is a data structure in system memory that contains various information in destination
//! to the host controller.
//!
//! The HCCA contains three things:
//!
//! - 32 endpoint lists. At each USB frame, the interrupt list whose index is `frame_number % 32`
//! is executed. These endpoint lists must contain only interrupt transfer descriptors, and end
//! with another endpoint list with only isochronous descriptors.
//!
//! - A frame number field written by the controller at the end of of each frame.
//!
//! - A "done queue" where all the transfer descriptors that have completed (successfully or not)
//! are pushed by the controller.
//!
//! After you have created an [`Hcca`], call [`Hcca::pointer`] and put the value in the `HcHcca`
//! register.
//!

use crate::{
    ohci::{ep_list, transfer_descriptor},
    Buffer32, HwAccessRef,
};

use alloc::vec::Vec;
use arrayvec::ArrayVec;
use core::{alloc::Layout, convert::TryFrom as _, num::NonZeroU32};

pub use transfer_descriptor::{CompletedTransferDescriptor, CompletionCode};

/// Manages the HCCA. See the module-level documentation.
pub struct Hcca<TAcc, TEpUd>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Access to the physical memory.
    hardware_access: TAcc,
    /// Contains the HCCA itself.
    buffer: Buffer32<TAcc>,
    /// The 32 interrupt lists. They all point to `isochronous_list`.
    interrupt_lists: ArrayVec<[ep_list::EndpointList<TAcc, TEpUd>; 32]>,
    /// Pointed by the interrupt lists. Must contain only isochronous descriptors.
    isochronous_list: ep_list::EndpointList<TAcc, TEpUd>,
    /// Latest known value of the `DoneHead` field. Compared against the actual value to
    /// determine whether it has been updated.
    latest_known_done_head: u32,
}

impl<TAcc, TEpUd> Hcca<TAcc, TEpUd>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Allocates a new [`Hcca`]. The `req_alignment` represents the memory alignment of the HCCA
    /// as required by the controller. It can be determined by writing all 1s to the `HcHCCA`
    /// register then reading the value back, as explained in section 7.2.1 of the specs.
    pub async fn new(hardware_access: TAcc, req_alignment: usize) -> Hcca<TAcc, TEpUd> {
        assert!(req_alignment >= 256);
        let buffer = Buffer32::new(
            hardware_access.clone(),
            Layout::from_size_align(256, req_alignment).unwrap(),
        )
        .await;

        // List of endpoints and the isochronous transfer descriptors attached to them.
        let isochronous_list = ep_list::EndpointList::new(hardware_access.clone(), true).await;

        // Initialize one endpoint list for each interrupt list.
        let interrupt_lists = {
            let mut interrupt_lists = ArrayVec::new();
            for n in 0..32 {
                let mut list = ep_list::EndpointList::new(hardware_access.clone(), false).await;
                unsafe {
                    list.set_next(&isochronous_list).await;
                    hardware_access
                        .write_memory_u32_le(
                            u64::from(buffer.pointer().get()) + 4 * n,
                            &[list.head_pointer().get()],
                        )
                        .await;
                }
                interrupt_lists.push(list);
            }
            interrupt_lists
        };

        // The rest of the HCAA is only written by the controller. We initialize it with 0s, just
        // in case.
        unsafe {
            hardware_access
                .write_memory_u8(u64::from(buffer.pointer().get()) + 0x80, &[0; 0x80])
                .await;
        }

        Hcca {
            hardware_access,
            buffer,
            interrupt_lists,
            isochronous_list,
            latest_known_done_head: 0,
        }
    }

    /// Returns the physical memory address of the HCCA.
    ///
    /// This value never changes and is valid until the [`Hcca`] is destroyed.
    pub fn pointer(&self) -> NonZeroU32 {
        self.buffer.pointer()
    }

    /// Returns the low 16 bits of the frame number.
    ///
    /// The host controller periodically writes this value in the HCCA. This function retrieves
    /// it.
    pub async fn frame_number(&self) -> u16 {
        unsafe {
            let mut out = [0, 0];
            self.hardware_access
                .read_memory_u8(u64::from(self.buffer.pointer().get() + 0x80), &mut out)
                .await;
            u16::from_le_bytes(out)
        }
    }

    /// Extracts the transfer descriptors from the done queue.
    ///
    /// Transfer descriptors that are finished execution are moved to the done queue. The returned
    /// descriptors are in the opposite order from the one in which they have completed.
    ///
    /// # Safety
    ///
    /// To avoid race conditions, you must only call this function while the `WritebackDoneHead`
    /// bit of `InterruptStatus` is set. You can clear the bit after this function returns.
    ///
    /// The user data must match the one that is used when pushing descriptors.
    pub async unsafe fn extract_done_queue<TUd>(
        &mut self,
    ) -> Vec<CompletedTransferDescriptor<TUd>> {
        // Read the value of the `DoneHead` field.
        let (done_head, _lsb_set) = {
            let mut out = [0];
            self.hardware_access
                .read_memory_u32_le(u64::from(self.buffer.pointer().get() + 0x84), &mut out)
                .await;
            let lsb_set = (out[0] & 0x1) == 1;
            out[0] &= !0x1;
            (out[0], lsb_set)
        };

        // TODO: do something with this lsb_set thing

        // If this value if the same as last time, we immediately return.
        // This pointer is stale, and it would be an undefined behaviour to read it.
        if done_head == self.latest_known_done_head {
            return Vec::new();
        }

        // Note that we update `self.latest_known_done_head` only we know that the extraction
        // has finished.
        let list =
            transfer_descriptor::extract_leaked(self.hardware_access.clone(), done_head).await;
        self.latest_known_done_head = done_head;
        list
    }
}
