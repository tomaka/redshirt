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
use core::convert::TryFrom as _;

/// Builds the data of a device request.
pub fn encode_request(request: &Request) -> Vec<u8> {
    let mut data_buf = Vec::with_capacity(8 + request.data.len());
    data_buf.resize(8, 0);
    data_buf[0] = {
        let dir = match request.direction {
            RequestDirection::HostToDevice => 0,
            RequestDirection::DeviceToHost => 1,
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
    data_buf[1] = request.b_request;
    data_buf[2..4].copy_from_slice(&request.w_value.to_le_bytes());
    data_buf[4..6].copy_from_slice(&request.w_index.to_le_bytes());
    data_buf[6..8].copy_from_slice(&u16::try_from(request.data.len()).unwrap().to_le_bytes());
    data_buf.extend_from_slice(request.data);
    data_buf
}

#[derive(Debug)]
pub struct Request<'a> {
    pub direction: RequestDirection,
    pub ty: RequestTy,
    pub recipient: RequestRecipient,
    // TODO: merge with `ty`?
    pub b_request: u8,
    pub w_value: u16,
    // TODO: stronger typing
    pub w_index: u16,
    pub data: &'a [u8],
}

#[derive(Debug)]
pub enum RequestDirection {
    HostToDevice,
    DeviceToHost,
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
            direction: RequestDirection::DeviceToHost,
            ty: RequestTy::Standard,
            recipient: RequestRecipient::Device,
            b_request: 0x6,
            w_value: 0,
            w_index: 0,
            // length: 18
            data: &[],
        }
    }

    /// Builds a request that asks the device to change its address.
    pub fn set_address(address: u16) -> Self {
        Request {
            direction: RequestDirection::HostToDevice,
            ty: RequestTy::Standard,
            recipient: RequestRecipient::Device,
            b_request: 0x5,
            w_value: address,
            w_index: 0,
            data: &[],
        }
    }
}
