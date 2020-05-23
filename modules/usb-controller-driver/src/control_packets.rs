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

//! Utility functions for building and decoding control packets.
//!
//! Control packets can be considered as "the metadata" of a USB device.

use alloc::vec::Vec;
use core::{convert::TryFrom as _, num::NonZeroU8};

/// Builds the data of a device request. Contains the SETUP packet and an optional DATA packet.
pub fn encode_request<'a>(request: Request<'a>) -> ([u8; 8], RequestDirection<'a>) {
    let mut header = [0; 8];
    header[0] = {
        let dir = match request.direction {
            RequestDirection::HostToDevice { .. } => 0,
            RequestDirection::DeviceToHost { .. } => 1,
        };

        let ty = match request.ty {
            RequestTy::Standard => 0,
            RequestTy::Class => 1,
            RequestTy::Vendor => 2,
        };

        let recipient = match request.recipient {
            RequestRecipient::Device => 0,
            RequestRecipient::Interface => 1,
            RequestRecipient::Endpoint => 2,
            RequestRecipient::Other => 3,
        };

        (dir << 7) | (ty << 5) | recipient
    };
    header[1] = request.b_request;
    header[2..4].copy_from_slice(&request.w_value.to_le_bytes());
    header[4..6].copy_from_slice(&request.w_index.to_le_bytes());
    header[6..8].copy_from_slice(
        &{
            match request.direction {
                RequestDirection::HostToDevice { data } => u16::try_from(data.len()).unwrap(),
                RequestDirection::DeviceToHost { buffer_len } => buffer_len,
            }
        }
        .to_le_bytes(),
    );

    (header, request.direction)
}

#[derive(Debug)]
pub struct Request<'a> {
    pub direction: RequestDirection<'a>,
    pub ty: RequestTy,
    pub recipient: RequestRecipient,
    // TODO: merge with `ty`?
    pub b_request: u8,
    pub w_value: u16,
    // TODO: stronger typing
    pub w_index: u16,
}

#[derive(Debug)]
pub enum RequestDirection<'a> {
    HostToDevice { data: &'a [u8] },
    DeviceToHost { buffer_len: u16 },
}

impl<'a> RequestDirection<'a> {
    pub fn into_out_data(&self) -> Option<&'a [u8]> {
        match self {
            RequestDirection::HostToDevice { data } => Some(data),
            _ => None,
        }
    }

    pub fn into_in_buffer_len(&self) -> Option<u16> {
        match self {
            RequestDirection::DeviceToHost { buffer_len } => Some(*buffer_len),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum RequestTy {
    Standard,
    Class,
    Vendor,
}

#[derive(Debug)]
pub enum RequestRecipient {
    Device,
    Interface,
    Endpoint,
    Other,
}

impl<'a> Request<'a> {
    /// Builds a request that queries the device for a descriptor.
    pub fn get_descriptor(descriptor_ty: u8) -> Self {
        Request {
            direction: RequestDirection::DeviceToHost { buffer_len: 18 },
            ty: RequestTy::Standard,
            recipient: RequestRecipient::Device,
            b_request: 0x6,
            w_value: 1 << 8,
            w_index: 0,
        }
    }

    /// Builds a request that asks the device to change its address.
    pub fn set_address(address: NonZeroU8) -> Self {
        Request {
            direction: RequestDirection::HostToDevice { data: &[] },
            ty: RequestTy::Standard,
            recipient: RequestRecipient::Device,
            b_request: 0x5,
            w_value: u16::from(address.get()),
            w_index: 0,
        }
    }
}
