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

use crate::HwAccessRef;

use alloc::alloc::handle_alloc_error;
use core::{alloc::Layout, marker::PhantomData};

/// A single endpoint descriptor.
///
/// This structure can be seen as a list of transfers that the USB controller must perform with
/// a specific endpoint. The endpoint descriptor has to be put in an appropriate list for any work
/// to be done.
///
/// Since this list might be accessed by the controller, appropriate thread-safety measures have
/// to be taken.
pub struct EndpointDescriptor<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Hardware abstraction layer.
    hardware_access: TAcc,
    /// Physical memory buffer containing the endpoint descriptor.
    buffer: u32,
}

/// Configuration when initialization an [`EndpointDescriptor`].
#[derive(Debug, Clone)]
pub struct Config {
    /// Maximum number of bytes that can be sent or received in a single data packet. Only used
    /// when the direction is `OUT` or `SETUP`. Must be inferior or equal to 4095.
    pub maximum_packet_size: u16,
    /// Value between 0 and 128. The USB address of the function containing the endpoint.
    pub function_address: u8,
    /// Value between 0 and 16. The USB address of the endpoint within the function.
    pub endpoint_number: u8,
    /// If true, isochronous TD format. If false, general TD format.
    pub isochronous: bool,
    /// If false, full speed. If true, low speed.
    pub low_speed: bool,
    /// Direction of the data flow.
    pub direction: Direction,
}

#[derive(Debug, Clone)]
pub enum Direction {
    In,
    Out,
    FromTd,
}

impl<TAcc> EndpointDescriptor<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Allocates a new endpoint descriptor buffer in physical memory.
    pub async fn new(hardware_access: TAcc, config: Config) -> EndpointDescriptor<TAcc> {
        let buffer = match hardware_access.alloc32(ENDPOINT_DESCRIPTOR_LAYOUT).await {
            Ok(b) => b,
            Err(_) => handle_alloc_error(ENDPOINT_DESCRIPTOR_LAYOUT), // TODO: return error instead
        };

        let header = EndpointControlDecoded {
            maximum_packet_size: config.maximum_packet_size,
            format: config.isochronous,
            skip: true,
            low_speed: config.low_speed,
            direction: config.direction,
            endpoint_number: config.endpoint_number,
            function_address: config.function_address,
        };

        unsafe {
            hardware_access
                .write_memory_u8(u64::from(buffer), &header.encode())
                .await;
            hardware_access.write_memory_u32(u64::from(buffer + 12), &[0]).await;
        }

        EndpointDescriptor {
            hardware_access,
            buffer,
        }
    }

    /// Sets the next endpoint descriptor in the linked list.
    ///
    /// Endpoint descriptors are always part of a linked list, where each descriptor points to the
    /// next one, or to nothing.
    pub async unsafe fn set_next<UAcc>(&self, next: &EndpointDescriptor<UAcc>)
    where
        for<'r> &'r UAcc: HwAccessRef<'r>,
    {
        unimplemented!()
    }

    /// Sets the next endpoint descriptor in the linked list to nothing.
    pub async fn clear_next(&self) {
        unsafe {
            self.hardware_access
                .write_memory_u32(u64::from(self.buffer + 12), &[0])
                .await;
        }
    }
}

impl<TAcc> EndpointDescriptor<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    fn drop(&mut self) {
        unsafe {
            self.hardware_access
                .dealloc(u64::from(self.buffer), true, ENDPOINT_DESCRIPTOR_LAYOUT);
        }
    }
}

const ENDPOINT_DESCRIPTOR_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(16, 16) };

#[derive(Debug)]
struct EndpointControlDecoded {
    /// Maximum number of bytes that can be sent or received in a single data packet. Only used
    /// when the direction is `OUT` or `SETUP`. Must be inferior or equal to 4095.
    maximum_packet_size: u16,
    /// If true, isochronous TD format. If false, general TD format.
    format: bool,
    /// When set, the HC continues on the next ED off the list without accessing this one.
    skip: bool,
    /// If false, full speed. If true, low speed.
    low_speed: bool,
    /// Direction of the data flow.
    direction: Direction,
    /// Value between 0 and 16. The USB address of the endpoint within the function.
    endpoint_number: u8,
    /// Value between 0 and 128. The USB address of the function containing the endpoint.
    function_address: u8,
}

impl EndpointControlDecoded {
    pub fn encode(&self) -> [u8; 4] {
        assert!(self.maximum_packet_size < (1 << 12));
        assert!(self.endpoint_number < (1 << 5));
        assert!(self.function_address < (1 << 7));

        let direction = match self.direction {
            Direction::In => 0b10,
            Direction::Out => 0b01,
            Direction::FromTd => 0b00,
        };

        let val = u32::from(self.maximum_packet_size) << 16
            | if self.format { 1 } else { 0 } << 15
            | if self.skip { 1 } else { 0 } << 14
            | if self.low_speed { 1 } else { 0 } << 13
            | direction << 11
            | u32::from(self.endpoint_number) << 7
            | u32::from(self.function_address);

        val.to_be_bytes()
    }
}
