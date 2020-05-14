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

use alloc::vec::Vec;
use core::{
    convert::TryFrom as _,
    num::{NonZeroU32, NonZeroU8},
};
use fnv::FnvBuildHasher;
use hashbrown::HashMap;

/// Manages the state of a collection of USB devices connected to a specific controller.
#[derive(Debug)]
pub struct UsbDevices {
    /// List of devices, sorted by address.
    devices: HashMap<NonZeroU32, Device, FnvBuildHasher>,

    /// State of the root ports. Port 1 is at index 0, port 2 at index 1, and so on.
    root_hub_ports: Vec<PortState>,
}

#[derive(Debug)]
struct Device {
    /// Address of the hub this device is connected to, or `None` if it is connected to the
    /// root hub.
    hub_address: Option<NonZeroU32>,
}

/// State of a port.
#[derive(Debug)]
enum PortState {
    Vacant,
    Connected,
    Resetting,
    ResetFinished,
    Enabling,
}

/// Action that should be performed by the controller.
#[derive(Debug)]
pub enum Action {
    ResetRootHubPort { port: NonZeroU8 },
    EnableRootHubPort { port: NonZeroU8 },
    DisableRootHubPort { port: NonZeroU8 },
}

impl UsbDevices {
    pub fn new(root_hub_ports: NonZeroU8) -> Self {
        UsbDevices {
            devices: HashMap::with_capacity_and_hasher(
                root_hub_ports.get().into(),
                Default::default(),
            ),
            root_hub_ports: (0..root_hub_ports.get())
                .map(|_| PortState::Vacant)
                .collect(),
        }
    }

    /// Sets whether there is a device connected to a root hub port.
    pub fn set_root_hub_connected(&mut self, root_hub_port: NonZeroU8, connected: bool) {
        let mut state = &mut self.root_hub_ports[usize::from(root_hub_port.get() - 1)];
        match (&mut state, connected) {
            (PortState::Vacant, true) => *state = PortState::Connected,
            (PortState::Connected, false) => *state = PortState::Vacant,
            _ => {}
        }
    }

    /// Asks the [`UsbDevices`] which action to perform next.
    pub fn next_action(&mut self) -> Option<Action> {
        // Start resetting a port, if possible.
        if let Some(p) = self
            .root_hub_ports
            .iter()
            .position(|p| matches!(p, PortState::Connected))
        {
            self.root_hub_ports[p] = PortState::Resetting;
            let port = NonZeroU8::new(u8::try_from(p + 1).unwrap()).unwrap();
            return Some(Action::ResetRootHubPort { port });
        }

        // Start enabling a port, if possible.
        if let Some(p) = self
            .root_hub_ports
            .iter()
            .position(|p| matches!(p, PortState::Connected))
        {
            let ready_to_enable = !self
                .root_hub_ports
                .iter()
                .any(|p| matches!(p, PortState::Enabling));
            if ready_to_enable {
                self.root_hub_ports[p] = PortState::Enabling;
                let port = NonZeroU8::new(u8::try_from(p + 1).unwrap()).unwrap();
                return Some(Action::EnableRootHubPort { port });
            }
        }

        None
    }
}
