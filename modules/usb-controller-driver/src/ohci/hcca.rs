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

use crate::{ohci::ep_list, Buffer32, HwAccessRef};

use arrayvec::ArrayVec;
use core::{alloc::Layout, num::NonZeroU32};

// TODO: implement the Done queue stuff

pub struct Hcca<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    buffer: Buffer32<TAcc>,
    interrupt_lists: ArrayVec<[ep_list::EndpointList<TAcc>; 32]>,
    isochronous_list: ep_list::EndpointList<TAcc>,
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

        let isochronous_list = ep_list::EndpointList::new(hardware_access.clone()).await;

        // Initialize one endpoint list for each interrupt list.
        let interrupt_lists = {
            let mut interrupt_lists = ArrayVec::new();
            for n in 0..32 {
                let mut list = ep_list::EndpointList::new(hardware_access.clone()).await;
                unsafe {
                    list.set_next(&isochronous_list).await;
                    hardware_access
                        .write_memory_u32_be(
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
            buffer,
            interrupt_lists,
            isochronous_list,
        }
    }

    /// Returns the physical memory address of the HCCA.
    ///
    /// This value never changes and is valid until the [`Hcca`] is destroyed.
    pub fn pointer(&self) -> NonZeroU32 {
        self.buffer.pointer()
    }
}
