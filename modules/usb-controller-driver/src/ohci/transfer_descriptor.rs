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

use alloc::{alloc::handle_alloc_error, vec};
use core::{alloc::Layout, convert::TryFrom as _, marker::PhantomData, mem, num::NonZeroU32, ptr};

/// Placeholder for a future transfer descriptor.
///
/// Contains a physical buffer allocation but without any meaning.
pub struct TransferDescriptorPlaceholder<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Hardware abstraction layer.
    hardware_access: TAcc,
    /// True if this is an isochronous descriptor.
    isochronous: bool,
    /// Physical memory buffer containing the transfer descriptor.
    descriptor: Buffer32<TAcc>,
}

impl<TAcc> TransferDescriptorPlaceholder<TAcc>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Allocates a new transfer descriptor buffer in physical memory.
    pub async fn new(
        hardware_access: TAcc,
        isochronous: bool,
    ) -> TransferDescriptorPlaceholder<TAcc> {
        let descriptor = {
            const GENERIC_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(16, 16) };
            const ISOCHRONOUS_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(32, 32) };
            let layout = if isochronous {
                ISOCHRONOUS_LAYOUT
            } else {
                GENERIC_LAYOUT
            };

            Buffer32::new(hardware_access.clone(), layout).await
        };

        TransferDescriptorPlaceholder {
            hardware_access,
            isochronous,
            descriptor,
        }
    }

    /// Returns the physical memory address of the descriptor.
    ///
    /// This value never changes and is valid until the [`TransferDescriptorPlaceholder`] is
    /// destroyed.
    pub fn pointer(&self) -> NonZeroU32 {
        self.descriptor.pointer()
    }

    /// Turns the prototype into an actual descriptor, then links to a new placeholder and returns
    /// that placeholder.
    ///
    /// # Context
    ///
    /// Transfer descriptors form a linked list that the controller reads and processes. Once
    /// a transfer descriptor has been completed, it is moved out by the controller to a separate
    /// queue.
    ///
    /// This function assumes that `self` is the tail of the linked list. The value returned
    /// corresponds to the new tail of the queue, while `self` is "leaked" with a `mem::forget`.
    ///
    /// The "leaked" descriptor can later be retreived by calling .
    // TODO: calling what?
    pub async fn build_and_leak<'a, TUd>(
        self,
        config: TransferDescriptorConfig<'a>,
        user_data: TUd,
    ) -> TransferDescriptorPlaceholder<TAcc> {
        // Check correct type of descriptor.
        match (&config, self.isochronous) {
            (TransferDescriptorConfig::GeneralOut { .. }, false) => (),
            (TransferDescriptorConfig::GeneralIn { .. }, false) => (),
            (TransferDescriptorConfig::Isochronous { .. }, true) => (),
            _ => panic!(),
        }

        // Size of the buffer that the USB controller will see.
        let base_buffer_len = match config {
            TransferDescriptorConfig::GeneralOut { data, .. } => data.len(),
            TransferDescriptorConfig::GeneralIn { buffer_len, .. } => buffer_len,
            TransferDescriptorConfig::Isochronous { .. } => unimplemented!(),
        };

        assert!(base_buffer_len < 4096);
        assert!(base_buffer_len >= 1);
        let base_buffer_len_u32 = u32::try_from(base_buffer_len).unwrap();

        // We allocate a buffer of data containing the request space or the data to send, plus a
        // trailing struct containing some user data.
        let data_buffer = {
            assert_eq!(mem::align_of::<Trailer<TUd>>(), 1);
            let total_buffer_len = base_buffer_len + mem::size_of::<Trailer<TUd>>();
            let total_buffer_len_u32 = u32::try_from(total_buffer_len).unwrap();

            let data_buffer = {
                let layout = Layout::from_size_align(total_buffer_len, 1).unwrap();
                Buffer32::new(self.hardware_access.clone(), layout).await
            };

            match config {
                // Nothing to do. Leave the buffer uninitialized.
                TransferDescriptorConfig::GeneralIn { .. } => {}
                // TODO:
                TransferDescriptorConfig::Isochronous { .. } => unimplemented!(),
                // Upload the data in the buffer.
                TransferDescriptorConfig::GeneralOut { data, .. } => unsafe {
                    self.hardware_access
                        .write_memory_u8(u64::from(data_buffer.pointer().get()), data)
                        .await;
                },
            }

            // Now let's upload the trailer at the end of the buffer.
            let trailer = Trailer {
                isochronous: self.isochronous,
                data_buffer_start: data_buffer.pointer().get(),
                user_data,
            };

            unsafe {
                let mut trailer_bytes = vec![0; mem::size_of_val(&trailer)];
                ptr::write_unaligned(trailer_bytes.as_mut_ptr() as *mut _, trailer);

                self.hardware_access
                    .write_memory_u8(
                        u64::from(
                            data_buffer
                                .pointer()
                                .get()
                                .checked_add(base_buffer_len_u32)
                                .unwrap(),
                        ),
                        &trailer_bytes,
                    )
                    .await;
            }

            data_buffer
        };

        // Header field.
        let header = match config {
            TransferDescriptorConfig::GeneralOut {
                setup,
                delay_interrupt,
                ..
            } => {
                assert!(delay_interrupt < 8);
                (u32::from(delay_interrupt) << 21) | (if setup { 0b00 } else { 0b01 } << 19)
            }
            TransferDescriptorConfig::GeneralIn {
                buffer_rounding,
                delay_interrupt,
                ..
            } => {
                assert!(delay_interrupt < 8);
                (u32::from(delay_interrupt) << 21)
                    | (if buffer_rounding { 1 } else { 0 } << 18)
                    | (0b10 << 19)
            }
            TransferDescriptorConfig::Isochronous { .. } => unimplemented!(),
        };

        // Now that the buffer is ready, allocate the next placeholder in the list.
        let new_placeholder =
            TransferDescriptorPlaceholder::new(self.hardware_access.clone(), self.isochronous)
                .await;
        debug_assert_eq!(new_placeholder.pointer().get() % 16, 0);

        // Write the actual descriptor.
        unsafe {
            self.hardware_access
                .write_memory_u32_be(
                    u64::from(self.descriptor.pointer().get()),
                    &[
                        header,
                        data_buffer.pointer().get(),
                        new_placeholder.pointer().get(),
                        data_buffer
                            .pointer()
                            .get()
                            .checked_add(base_buffer_len_u32.checked_sub(1u32).unwrap())
                            .unwrap(),
                    ],
                )
                .await;
        }

        // Now leak ourself and return the new queue tail, as explained in this function's
        // documentation.
        mem::forget(self);
        new_placeholder
    }
}

/// We append the following trailer after each buffer containing data, for later identification.
#[repr(packed)]
struct Trailer<TUd> {
    isochronous: bool,
    /// Physical memory address of the start of the buffer.
    data_buffer_start: u32,
    user_data: TUd,
}

/// Configuration for a transfer descriptor.
#[derive(Debug)]
pub enum TransferDescriptorConfig<'a> {
    /// Control, bulk, or interrupt transfer descriptor that sends data out.
    GeneralOut {
        /// Data to send out.
        data: &'a [u8],
        /// Use a `SETUP` PID rather than `OUT`.
        setup: bool,
        /// Number of frames between the end of the transmission and the interrupt triggering.
        delay_interrupt: u8,
    },

    /// Control, bulk, or interrupt transfer descriptor that receives data.
    GeneralIn {
        /// Size in bytes of the buffer that receives the data.
        buffer_len: usize,
        /// If true, `buffer_len` must exactly match the length of the data sent by the endpoint,
        /// otherwise an error happens.
        buffer_rounding: bool,
        /// Number of frames between the end of the transmission and the interrupt triggering.
        delay_interrupt: u8,
    },

    /// Isochronous transfer descriptor.
    Isochronous {
        /// Lower 16bits of the frame number at which to start processing this isochronous buffer.
        /// This endpoint descriptor is entirely skipped if it starts with a transfer descriptor
        /// whose starting frame is inferior to the current frame.
        starting_frame: u16,
        /// Number of frames between the end of the transmission and the interrupt triggering.
        delay_interrupt: u8,
        // TODO: not finished
    },
}
