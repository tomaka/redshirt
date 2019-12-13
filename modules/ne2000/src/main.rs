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

//! Driver for the ne2000 network card.
//!
//! This program scans the PCI space for the ne2000. If it finds it, it registers a new network
//! interface towards the network manager, and handles the communication between the network
//! manager and the hardware.
//!
//! Bibliography:
//!
//! - https://wiki.osdev.org/Ne2000
//! - https://en.wikipedia.org/wiki/NE1000#NE2000
//! - http://www.ethernut.de/pdf/8019asds.pdf
//!

mod device;

use std::convert::TryFrom as _;

fn main() {
    nametbd_syscalls_interface::block_on(async_main());
}

async fn async_main() {
    unsafe {
        device::Device::reset(0xc001).await;        // TODO: don't hardcode
        nametbd_stdout_interface::stdout(format!("Initialized ne2000"));
    }
}
