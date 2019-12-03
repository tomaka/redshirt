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

use smoltcp::iface::{EthernetInterface, EthernetInterfaceBuilder, NeighborCache};
use smoltcp::wire::EthernetAddress;
use std::{collections::BTreeMap, time::Duration};

mod raw_device;

pub struct RegisteredDevice {
    ethernet: EthernetInterface<'static, 'static, 'static, raw_device::RawDevice>,
}

impl RegisteredDevice {
    pub fn new() -> RegisteredDevice {
        let device = raw_device::RawDevice::new();

        let interface = EthernetInterfaceBuilder::new(device)
            .ethernet_addr(EthernetAddress([0x01, 0x00, 0x00, 0x00, 0x00, 0x02]))       // TODO:
            .neighbor_cache(NeighborCache::new(BTreeMap::new()))
            .finalize();

        RegisteredDevice {
            ethernet: interface
        }
    }

    pub async fn next_event(&mut self, sockets: &mut smoltcp::socket::SocketSet<'static, 'static, 'static>) {
        loop {
            let next_poll: Duration = match self.ethernet.poll_delay(sockets, now().await) {
                Some(d) => d.into(),
                None => {
                    futures::pending!();
                    continue;
                }
            };

            nametbd_time_interface::monotonic_wait(next_poll).await;

            self.ethernet.poll(sockets, now().await);
        }
    }
}

async fn now() -> smoltcp::time::Instant {
    let now = nametbd_time_interface::monotonic_clock().await;
    smoltcp::time::Instant::from_millis((now / 1_000_000) as i64)       // TODO: don't use as
}
