// Copyright (C) 2019-2021  Pierre Krieger
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

use crate::{ohci::transfer_descriptor, Buffer32, HwAccessRef};

use alloc::vec::Vec;
use core::{alloc::Layout, num::NonZeroU32};

pub use transfer_descriptor::{CompletedTransferDescriptor, TransferDescriptorConfig};

/// A single endpoint descriptor.
///
/// This structure can be seen as a list of transfers that the USB controller must perform with
/// a specific endpoint. The endpoint descriptor has to be put in an appropriate list for any work
/// to happen.
///
/// Since this list might be accessed by the controller, appropriate thread-safety measures have
/// to be taken.
//
// # Queueing a new transfer descriptor
//
// The endpoint descriptor points to the head and tail of a linked list of transfer descriptors.
// Each transfer descriptor points to the next one in the list.
//
// In order to avoid synchronization issues, the controller never processes the last element in
// the list. In other words, if it reaches the transfer descriptor pointed to by the tail, it
// doesn't process it.
// This means that this last transfer descriptor can be used as a place-holder for the next actual
// transfer descriptor. Once this placeholder has been filled with actual value, we push a
// follow-up dummy descriptor and update the tail.
//
// Removing pending transfer descriptors, however, can only be done by pausing execution and
// making sure the controller is done accessing the transfer descriptor.
//
pub struct EndpointDescriptor<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Hardware abstraction layer.
    hardware_access: TAcc,
    /// Physical memory buffer containing the endpoint descriptor.
    buffer: Buffer32<TAcc>,
    /// Placeholder for the next transfer descriptor. Should always be `Some`. Moved out only
    /// temporarily.
    next_transfer_descriptor: Option<transfer_descriptor::TransferDescriptorPlaceholder<TAcc>>,
    /// Value that was passed to `new`. Never modified.
    isochronous: bool,
    /// Direction value that was passed to `new`. Never modified.
    direction: Direction,
    /// Maximum packet size that was passed to `new`. Never modified.
    maximum_packet_size: u16,
}

/// Configuration when initialization an [`EndpointDescriptor`].
#[derive(Debug, Clone)]
pub struct Config {
    /// Maximum number of bytes that can be sent or received in a single data packet. Only used
    /// when the direction is `OUT` or `SETUP`. Must be inferior or equal to 4095.
    /// Unused when `isochronous` is true.
    // TODO: use an enum or something?
    pub maximum_packet_size: u16,
    /// Value between 0 and 127. The USB address of the function containing the endpoint.
    pub function_address: u8,
    /// Value between 0 and 15. The index of the endpoint within the function.
    pub endpoint_number: u8,
    /// If true, isochronous TD format. If false, general TD format.
    pub isochronous: bool,
    /// If false, full speed. If true, low speed.
    pub low_speed: bool,
    /// Direction of the data flow.
    pub direction: Direction,
}

/// Direction of the data flow.
#[derive(Debug, Copy, Clone)]
pub enum Direction {
    /// All the transfer descriptors have to receive data.
    In,
    /// All the transfer descriptors have to send data.
    Out,
    /// The direction is determined individually for each transfer descriptor.
    FromTd,
}

impl<TAcc> EndpointDescriptor<TAcc>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Allocates a new endpoint descriptor buffer in physical memory.
    pub async fn new(hardware_access: TAcc, config: Config) -> EndpointDescriptor<TAcc> {
        let buffer = {
            const ENDPOINT_DESCRIPTOR_LAYOUT: Layout =
                unsafe { Layout::from_size_align_unchecked(16, 16) };
            Buffer32::new(hardware_access.clone(), ENDPOINT_DESCRIPTOR_LAYOUT).await
        };

        let header = {
            assert!(config.maximum_packet_size < (1 << 12));
            assert!(config.endpoint_number < (1 << 5));
            assert!(config.function_address < (1 << 7));

            let direction = match config.direction {
                Direction::In => 0b10,
                Direction::Out => 0b01,
                Direction::FromTd => 0b00,
            };

            u32::from(config.maximum_packet_size) << 16
                | if config.isochronous { 1 } else { 0 } << 15
                | if config.low_speed { 1 } else { 0 } << 13
                | direction << 11
                | u32::from(config.endpoint_number) << 7
                | u32::from(config.function_address)
        };

        let next_transfer_descriptor = transfer_descriptor::TransferDescriptorPlaceholder::new(
            hardware_access.clone(),
            config.isochronous,
        )
        .await;

        unsafe {
            hardware_access
                .write_memory_u32_le(
                    u64::from(buffer.pointer().get()),
                    &[
                        header,
                        next_transfer_descriptor.pointer().get(), // Transfer descriptor tail
                        next_transfer_descriptor.pointer().get(), // Transfer descriptor head
                        0x0,                                      // Next endpoint descriptor
                    ],
                )
                .await;
        }

        EndpointDescriptor {
            hardware_access,
            buffer,
            next_transfer_descriptor: Some(next_transfer_descriptor),
            isochronous: config.isochronous,
            direction: config.direction,
            maximum_packet_size: config.maximum_packet_size,
        }
    }

    /// Returns the physical memory address of the descriptor.
    ///
    /// This value never changes and is valid until the [`EndpointDescriptor`] is destroyed.
    pub fn pointer(&self) -> NonZeroU32 {
        self.buffer.pointer()
    }

    /// Clears the `halted` flag.
    ///
    /// Some kind of errors during transfer descriptor operations will lead to the controller
    /// setting the `halted` flag on the endpoint descriptor. This flag is equivalent to having
    /// `skip` set to `true`.
    /// In order to resume operation, one should call this method.
    ///
    /// Returns `Ok(())` if the halted flag was indeed set, or `Err(())` if it was not.
    ///
    /// # Panic
    ///
    /// Panics if this is an isochronous endpoint descriptor. While calling this method on an
    /// isochronous endpoint descriptor wouldn't result in any bug or abnormal behaviour, it
    /// doesn't make sense to do so, and this panic is in place in order to detect mistakes.
    ///
    pub async fn clear_halted(&mut self) -> Result<(), ()> {
        unsafe {
            assert!(!self.isochronous);

            let mut out = [0];
            self.hardware_access
                .read_memory_u32_le(u64::from(self.buffer.pointer().get()) + 8, &mut out)
                .await;

            if (out[0] & 0b1) != 0 {
                // We only write back if the flag was actually set, otherwise we might trigger a
                // race condition with the controller.
                out[0] &= !0b1;
                self.hardware_access
                    .write_memory_u32_le(u64::from(self.buffer.pointer().get()) + 8, &out)
                    .await;
                Ok(())
            } else {
                Err(())
            }
        }
    }

    /// Sets the `skip` flag on the header to true or false.
    ///
    /// If `true`, this will cause the controller to completely ignore this descriptor.
    pub async fn set_skip(&mut self, skip: bool) {
        unsafe {
            let mut out = [0];
            self.hardware_access
                .read_memory_u32_le(u64::from(self.buffer.pointer().get()), &mut out)
                .await;
            if skip {
                out[0] |= (1 << 14);
            } else {
                out[0] &= !(1 << 14);
            }
            self.hardware_access
                .write_memory_u32_le(u64::from(self.buffer.pointer().get()), &out)
                .await;
        }
    }

    /// Destroys the [`EndpointDescriptor`] and returns the transfer descriptors that haven't been
    /// processed.
    ///
    /// # Safety
    ///
    /// To avoid race conditions, you must only call this function after the endpoint descriptor
    /// is no longer in use by the controller.
    ///
    /// The user data must match the one that was used when pushing descriptors.
    pub async unsafe fn destroy<TUd>(self) -> Vec<CompletedTransferDescriptor<TUd>> {
        let queue_head = {
            let mut out = [0];
            self.hardware_access
                .read_memory_u32_le(u64::from(self.buffer.pointer().get() + 8), &mut out)
                .await;
            // Clear halted and carry flags.
            out[0] &= !0b11;
            out[0]
        };

        // FIXME: wrong; will attempt to extract the dummy descriptor
        unimplemented!();
        transfer_descriptor::extract_leaked(self.hardware_access.clone(), queue_head).await
    }

    /// Pushes a new packet at the end of the list of transfer descriptors.
    ///
    /// After this packet has been processed by the controller, it will be moved to the "done
    /// queue" of the HCCA where you will be able to figure out whether the transfer worked.
    ///
    /// # Panic
    ///
    /// Panics if the type (isochronous or not) of transfer descriptor doesn't match the type of
    /// the endpoint descriptor.
    /// Panics if the direction of the transfer descriptor isn't compatible with the one passed
    /// in the configuration at initialization.
    /// Panics if the data buffer length is superior to the maximum packet size passed at
    /// initialization.
    ///
    pub async fn push_packet<'a, TUd: 'static>(
        &mut self,
        cfg: TransferDescriptorConfig<'a>,
        user_data: TUd,
    ) {
        // Check correctness of the operation.
        match (&cfg, self.isochronous, self.direction) {
            (TransferDescriptorConfig::GeneralOut { data, .. }, false, Direction::FromTd)
            | (TransferDescriptorConfig::GeneralOut { data, .. }, false, Direction::Out) => {
                assert!(data.len() <= usize::from(self.maximum_packet_size));
            }
            (TransferDescriptorConfig::GeneralIn { buffer_len, .. }, false, Direction::FromTd)
            | (TransferDescriptorConfig::GeneralIn { buffer_len, .. }, false, Direction::In) => {
                assert!(*buffer_len <= usize::from(self.maximum_packet_size));
            }
            (TransferDescriptorConfig::Isochronous { .. }, true, _) => {}
            _ => panic!(),
        }

        // TODO: the code below is not "futures-cancellation-safe" and will panic if one of the
        // awaits gets interrupted

        // Write `cfg` over `next_transfer_descriptor` and return the new queue tail.
        let new_placeholder = self
            .next_transfer_descriptor
            .take()
            .unwrap()
            .build_and_leak(cfg, user_data)
            .await;

        // Update the tail to the new placeholder.
        unsafe {
            self.hardware_access
                .write_memory_u32_le(
                    u64::from(self.buffer.pointer().get() + 4),
                    &[new_placeholder.pointer().get()],
                )
                .await;
        }

        self.next_transfer_descriptor = Some(new_placeholder);
    }

    /// Returns the value of the next endpoint descriptor in the linked list.
    ///
    /// If [`EndpointDescriptor::set_next`] or [`EndpointDescriptor::set_next_raw`] was previously
    /// called, returns the corresponding physical memory pointer. If
    /// [`EndpointDescriptor::clear_next`]
    pub async fn get_next_raw(&self) -> u32 {
        unsafe {
            let mut out = [0];
            self.hardware_access
                .read_memory_u32_le(u64::from(self.buffer.pointer().get() + 12), &mut out)
                .await;
            out[0]
        }
    }

    /// Sets the next endpoint descriptor in the linked list.
    ///
    /// Endpoint descriptors are always part of a linked list, where each descriptor points to the
    /// next one, or to nothing.
    ///
    /// # Safety
    ///
    /// `next` must remain valid until the next time [`EndpointDescriptor::clear_next`],
    /// [`EndpointDescriptor::set_next`] or [`EndpointDescriptor::set_next_raw`] is called, or
    /// until this [`EndpointDescriptor`] is destroyed.
    pub async unsafe fn set_next<UAcc>(&mut self, next: &EndpointDescriptor<UAcc>)
    where
        UAcc: Clone,
        for<'r> &'r UAcc: HwAccessRef<'r>,
    {
        self.set_next_raw(next.pointer().get()).await;
    }

    /// Sets the next endpoint descriptor in the linked list.
    ///
    /// If 0 is passed, has the same effect as [`EndpointDescriptor::clear_next`].
    ///
    /// # Safety
    ///
    /// If not 0, `next` must be the physical memory address of an endpoint descriptor. It must
    /// remain valid until the next time [`EndpointDescriptor::clear_next`],
    /// [`EndpointDescriptor::set_next`] or [`EndpointDescriptor::set_next_raw`] is called, or
    /// until this [`EndpointDescriptor`] is destroyed.
    pub async unsafe fn set_next_raw(&mut self, next: u32) {
        self.hardware_access
            .write_memory_u32_le(u64::from(self.buffer.pointer().get() + 12), &[next])
            .await;
    }

    /// Sets the next endpoint descriptor in the linked list to nothing.
    pub async fn clear_next(&mut self) {
        unsafe {
            self.set_next_raw(0).await;
        }
    }
}
