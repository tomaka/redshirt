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

//! Registers a network interface that uses [TAP](https://en.wikipedia.org/wiki/TAP_(network_driver)).

pub struct TapNetworkInterface {
    interface: tun_tap::Iface,
}

impl TapNetworkInterface {
    pub fn new() -> TapNetworkInterface {
        let interface = tun_tap::Iface::new("redshirt-%d", tun_tap::Mode::Tap).unwrap();     // TODO: don't unwrap
        TapNetworkInterface {
            interface
        }
    }
}
