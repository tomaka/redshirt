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

use crate::{Buffer32, HwAccessRef};

use alloc::alloc::handle_alloc_error;
use core::{alloc::Layout, convert::TryFrom as _, marker::PhantomData, num::NonZeroU32};

/// A single transfer descriptor, either general or isochronous.
///
/// This structure can be seen as a transfer that the USB controller must perform with a specific
/// endpoint. It has to be put in an appropriate endpoint list in order to work.
///
/// Since this list might be accessed by the controller, appropriate thread-safety measures have
/// to be taken.
pub struct TransferDescriptor<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Hardware abstraction layer.
    hardware_access: TAcc,
    /// Physical memory buffer containing the transfer descriptor.
    descriptor: Buffer32<TAcc>,
    /// Physical memory buffer containing the buffer that contains or will contain the USB packet.
    data_buffer: Buffer32<TAcc>,
}

impl<TAcc> TransferDescriptor<TAcc>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Allocates a new transfer descriptor buffer in physical memory.
    pub async fn new(hardware_access: TAcc) -> TransferDescriptor<TAcc> {
        let data_buffer_len = 0x1000u32;

        assert!(data_buffer_len > 0);
        assert!(data_buffer_len < 8192);

        let descriptor = {
            const TRANSFER_DESCRIPTOR_LAYOUT: Layout =
                unsafe { Layout::from_size_align_unchecked(16, 16) };
            Buffer32::new(hardware_access.clone(), TRANSFER_DESCRIPTOR_LAYOUT).await
        };

        let data_buffer = {
            // TODO: too strict alignment; a single page boundary is allowed
            let layout =
                Layout::from_size_align(usize::try_from(data_buffer_len).unwrap(), 4096).unwrap();
            Buffer32::new(hardware_access.clone(), layout).await
        };

        unsafe {
            hardware_access
                .write_memory_u32_be(
                    u64::from(descriptor.pointer().get()),
                    &[
                        0x0, // Header
                        data_buffer.pointer().get(),
                        0x0, // Next transfer descriptor
                        data_buffer
                            .pointer()
                            .get()
                            .checked_add(data_buffer_len.checked_sub(1).unwrap())
                            .unwrap(),
                    ],
                )
                .await;
        }

        TransferDescriptor {
            hardware_access,
            descriptor,
            data_buffer,
        }
    }

    /// Returns the physical memory address of the descriptor.
    ///
    /// This value never changes and is valid until the [`TransferDescriptor`] is destroyed.
    pub fn pointer(&self) -> NonZeroU32 {
        self.descriptor.pointer()
    }
}
