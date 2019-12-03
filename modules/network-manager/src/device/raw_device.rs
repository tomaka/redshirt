// Copyright (C) 2019  Pierre Krieger
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

use smoltcp::{phy, time::Instant};

/// Implementation of `smoltcp::phy::Device`.
pub struct RawDevice {

}

impl RawDevice {
    pub fn new() -> RawDevice {
        RawDevice {}
    }
}

impl<'a> smoltcp::phy::Device<'a> for RawDevice {
    type RxToken = RawDeviceRxToken<'a>;
    type TxToken = RawDeviceTxToken<'a>;

    fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
        None
    }

    fn transmit(&'a mut self) -> Option<Self::TxToken> {
        None
    }

    fn capabilities(&self) -> phy::DeviceCapabilities {
        let mut caps: phy::DeviceCapabilities = Default::default();
        caps.max_transmission_unit = 9216;        // FIXME:
        caps.max_burst_size = None;
        caps.checksum = phy::ChecksumCapabilities::ignored();
        caps
    }
}

pub struct RawDeviceRxToken<'a> {
    marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> phy::RxToken for RawDeviceRxToken<'a> {
    fn consume<R, F>(self, timestamp: Instant, f: F) -> Result<R, smoltcp::Error>
    where
        F: FnOnce(&[u8]) -> Result<R, smoltcp::Error>
    {
        unimplemented!()
    }
}

pub struct RawDeviceTxToken<'a> {
    marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> phy::TxToken for RawDeviceTxToken<'a> {
    fn consume<R, F>(self, timestamp: Instant, len: usize, f: F) -> Result<R, smoltcp::Error>
    where
        F: FnOnce(&mut [u8]) -> Result<R, smoltcp::Error>
    {
        unimplemented!()
    }
}
