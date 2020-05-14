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

use crate::{
    ohci::{ep_list, transfer_descriptor},
    Buffer32, HwAccessRef,
};

use alloc::vec::Vec;
use arrayvec::ArrayVec;
use core::{alloc::Layout, convert::TryFrom as _, num::NonZeroU32};

pub use transfer_descriptor::{CompletedTransferDescriptor, CompletionCode};

pub struct Hcca<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    hardware_access: TAcc,
    buffer: Buffer32<TAcc>,
    interrupt_lists: ArrayVec<[ep_list::EndpointList<TAcc>; 32]>,
    isochronous_list: ep_list::EndpointList<TAcc>,
    /// Latest known value of the `DoneHead` field. Used to check whether it has been updated.
    latest_known_done_head: u32,
}

impl<TAcc> Hcca<TAcc>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    pub async fn new(hardware_access: TAcc, req_alignment: usize) -> Hcca<TAcc> {
        assert!(req_alignment >= 256);
        let buffer = Buffer32::new(
            hardware_access.clone(),
            Layout::from_size_align(256, req_alignment).unwrap(),
        )
        .await;

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
    /// bit of `InterruptStatus` is set. You can then clear the bit.
    ///
    /// The user data must match the one that is used when pushing descriptors.
    ///
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

        // TODO: do this lsb_set thing

        // If this value if the same as last time, we immediately return.
        // This pointer is stale, as it would be undefined behaviour to read it.
        if done_head == self.latest_known_done_head {
            return Vec::new();
        }
        self.latest_known_done_head = done_head;

        transfer_descriptor::extract_leaked(self.hardware_access.clone(), done_head).await
    }
}
