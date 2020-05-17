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

//! Manages the state of a collection of USB devices connected to a specific controller.
//!
//! Also manages the ports of said controller.
//!
//! # Overview of USB
//!
//! A USB host controller features a list of ports, to each of which a USB device can potentially
//! be connected.
//!
//! The role of the USB host controller is to allow sending to or receiving packets from these
//! USB devices.
//!
//! When you send or receive a packet, you must specify the *address* of the USB device that must
//! receive or send this packet.
//! USB devices, when they connect, are by default assigned to address 0. This address must then
//! be reconfigured must by the operating system during the initialization process of the device.
//! At any given time, no two devices must be assigned to the same address. In order to enforce
//! this, devices must be enabled one by one and assigned an address one by one.
//!
//! Similar to an Ethernet hub, when you send or receive packet on a USB host controller, this
//! packet.is broadcasted to all the ports of the controller and the only device whose address
//! corresponds to the one in the packet processes it.
//!
//! Once a device has been assigned an address, it must be configured. TODO: finish

use crate::PortState;

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
    root_hub_ports: Vec<LocalPortState>,
}

#[derive(Debug)]
struct Device {
    /// Address of the hub this device is connected to, or `None` if it is connected to the
    /// root hub.
    hub_address: Option<NonZeroU32>,
}

/// State of a port.
// TODO: redundant with PortState at the root? unfortunately not because of the address
#[derive(Debug)]
enum LocalPortState {
    Disconnected,
    /// Port is connected to a device but is disabled.
    Disabled,
    Resetting,
    /// Port is enabled and is connected to a device for which no address has been assigned yet.
    EnabledDefaultAddress,
    /// Port contains a device with the given address.
    Address(NonZeroU32),
}

/// Action that should be performed by the controller.
#[derive(Debug)]
pub enum Action {
    SetRootHubPortState { port: NonZeroU8, state: PortState },
}

impl UsbDevices {
    pub fn new(root_hub_ports: NonZeroU8) -> Self {
        UsbDevices {
            devices: HashMap::with_capacity_and_hasher(
                root_hub_ports.get().into(),
                Default::default(),
            ),
            root_hub_ports: (0..root_hub_ports.get())
                .map(|_| LocalPortState::Disconnected)
                .collect(),
        }
    }

    /// Updates the [`UsbDevices`] with the state of a root hub port.
    ///
    /// # Panic
    ///
    /// Panics if the new state doesn't make sense when compared to the old state. In particular,
    /// the state machine of the [`UsbDevices`] assumes that it has exclusive control over the
    /// port (by generation [`Action::SetRootHubPortState`]). Shared ownership isn't (and can't
    /// be) supported.
    pub fn set_root_hub_port_state(&mut self, root_hub_port: NonZeroU8, new_state: PortState) {
        let mut state = &mut self.root_hub_ports[usize::from(root_hub_port.get() - 1)];
        match (&mut state, new_state) {
            (LocalPortState::Disconnected, PortState::Disabled) => {
                *state = LocalPortState::Disabled
            }
            (LocalPortState::Disabled, PortState::Disconnected)
            | (LocalPortState::Disabled, PortState::NotPowered)
            | (LocalPortState::Resetting, PortState::Disconnected)
            | (LocalPortState::Resetting, PortState::NotPowered) => {
                *state = LocalPortState::Disconnected
            }
            (LocalPortState::Resetting, PortState::Enabled) => {
                // Resetting the port has completed.
                *state = LocalPortState::EnabledDefaultAddress;
            }
            _ => panic!(),
        }
    }

    /// Asks the [`UsbDevices`] which action to perform next.
    pub fn next_action(&mut self) -> Option<Action> {
        // Start resetting a port, if possible.
        if let Some(p) = self
            .root_hub_ports
            .iter()
            .position(|p| matches!(p, LocalPortState::Disabled))
        {
            self.root_hub_ports[p] = LocalPortState::Resetting;
            let port = NonZeroU8::new(u8::try_from(p + 1).unwrap()).unwrap();
            return Some(Action::SetRootHubPortState {
                port,
                state: PortState::Resetting,
            });
        }

        None
    }
}
