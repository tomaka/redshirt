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
//! When you send or receive a packet, you must specify the *address* of the USB device (also
//! called a "function") that must receive or send this packet.
//! USB devices, when they connect and are enabled, are by default assigned to address 0. This
//! address must then be reconfigured by the operating system during the initialization process of
//! the device.
//! At any given time, no two devices must be assigned to the same address. In order to enforce
//! this, devices must be enabled one by one and assigned an address one by one.
//!
//! In addition to the address, sending a packet must also specify an *endpoint* number. Each USB
//! device/function consists of one or more endpoints. Endpoint zero is hardcoded to be the
//! *default control pipe* and is always available. The other endpoints depend on the way the
//! device is configured (see below).
//!
//! Similar to an Ethernet hub, when you send or receive packet on a USB host controller, this
//! packet.is broadcasted to all the ports of the controller and the only device whose address
//! corresponds to the one in the packet processes it.
//!
//! Once a device has been assigned an address, it must be configured. TODO: finish

use crate::{control_packets, EndpointTy, PortState};

use alloc::vec::Vec;
use core::{
    convert::TryFrom as _,
    num::{NonZeroU32, NonZeroU8},
    time::Duration,
};
use fnv::FnvBuildHasher;
use hashbrown::HashMap;

/// Manages a collection of USB devices connected to a specific controller.
#[derive(Debug)]
pub struct UsbDevices {
    /// If false, we have to report an action that allocates the default control pipe on the
    /// default address.
    allocated_default_endpoint: bool,

    /// Packet ID of the next packet.
    /// This is an internal identifier that is never communicated to USB devices.
    next_packet_id: PacketId,

    /// List of devices, sorted by address.
    devices: HashMap<NonZeroU8, Device, FnvBuildHasher>,

    /// State of the root ports. Port 1 is at index 0, port 2 at index 1, and so on.
    root_hub_ports: Vec<LocalPortState>,
}

#[derive(Debug)]
struct Device {
    configuration_state: ConfigurationState,

    /// Address of the hub this device is connected to, or `None` if it is connected to the
    /// root hub.
    hub_address: Option<NonZeroU8>,
}

#[derive(Debug)]
enum ConfigurationState {
    /// We have sent a `SET_ADDRESS` packet to the device, and we are now waiting a bit of time
    /// for it to start responding on the new address.
    WaitingAddressResponsive {
        /// Identifier for the wait for IP purposes. `None` if we haven't sent it out yet.
        wait: Option<PacketId>,
    },
    EndpointNotAllocated,
    NotConfigured,
    /// We have sent a request to the device asking for its device descriptor.
    DeviceDescriptorRequested(PacketId),
    Configured,
}

/// State of a port.
// TODO: redundant with PortState at the root? unfortunately not, because of the address
#[derive(Debug)]
enum LocalPortState {
    /// Corresponds to both [`PortState::NotPowered`] and [`PortState::Disconnected`].
    Disconnected,
    /// Port is connected to a device but is disabled.
    Disabled,
    Resetting,
    /// Port is enabled and is connected to a device for which no address has been assigned yet.
    EnabledDefaultAddress,
    /// Port is enabled and is connected to a device for which no address has been assigned yet.
    EnabledSendingAddress {
        packet_id: PacketId,
        address: NonZeroU8,
    },
    /// Port contains a device with the given address.
    Address(NonZeroU8),
}

/// Opaque packet identifier. Assigned by the [`UsbDevices`]. Identifies a packet that has been
/// emitted or requested through an [`Action`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PacketId(pub u64);

/// Action that should be performed by the controller.
#[derive(Debug)]
pub enum Action {
    /// Change the state of a port of the root hub.
    SetRootHubPortState {
        /// Port number.
        port: NonZeroU8,
        /// State to transition to. Guaranteed to be valid based on the latest call to
        /// [`UsbDevices::set_root_hub_port_state`].
        state: PortState,
    },

    /// Signals that the given endpoint on the given address will start being used.
    AllocateNewEndpoint {
        /// Value between 0 and 127. The USB address of the function containing the endpoint.
        ///
        /// > **Note**: The word "function" is synonymous with "device".
        function_address: u8,
        /// Value between 0 and 16. The index of the endpoint within the function.
        endpoint_number: u8,
        /// Type of the endpoint.
        ty: EndpointTy,
    },

    /// Signals that the given endpoint on the given address will no longer be used.
    FreeEndpoint {
        /// Value between 0 and 127. The USB address of the function containing the endpoint.
        ///
        /// > **Note**: The word "function" is synonymous with "device".
        function_address: u8,
        /// Value between 0 and 16. The index of the endpoint within the function.
        endpoint_number: u8,
    },

    /// Requests data from an endpoint.
    ///
    /// The endpoint will first have been reported with an [`Action::AllocateNewEndpoint`].
    EmitInPacket {
        /// Identifier assigned by the [`UsbDevices`]. Must be passed back later when calling
        /// [`UsbDevices::in_packet_result`].
        id: PacketId,
        /// Value between 0 and 127. The USB address of the function containing the endpoint.
        ///
        /// > **Note**: The word "function" is synonymous with "device".
        function_address: u8,
        /// Value between 0 and 16. The index of the endpoint within the function.
        endpoint_number: u8,
        /// Length of the buffer that the device is allowed to write to.
        buffer_len: u16,
    },

    /// Emits a packet to an endpoint.
    ///
    /// The endpoint will first have been reported with an [`Action::AllocateNewEndpoint`].
    EmitOutPacket {
        /// Identifier assigned by the [`UsbDevices`]. Must be passed back later when calling
        /// [`UsbDevices::out_packet_result`].
        id: PacketId,
        /// Value between 0 and 127. The USB address of the function containing the endpoint.
        ///
        /// > **Note**: The word "function" is synonymous with "device".
        function_address: u8,
        /// Value between 0 and 16. The index of the endpoint within the function.
        endpoint_number: u8,
        /// Data to be sent to the device.
        data: Vec<u8>,
    },

    /// Emits a `SETUP` packet followed with an incoming `DATA` packet to an endpoint. The endpoint
    /// must be of type [`EndpointTy::Control`].
    ///
    /// The endpoint will first have been reported with an [`Action::AllocateNewEndpoint`].
    ///
    /// > **Note**: This corresponds to the start of a device-to-host control transfer. After
    /// >           receiving the data, the [`UsbDevices`] will emit an [`Action::EmitOutPacket`]
    /// >           for the status packet.
    EmitSetupInPacket {
        /// Identifier assigned by the [`UsbDevices`]. Must be passed back later when calling
        /// [`UsbDevices::in_packet_result`].
        id: PacketId,
        /// Value between 0 and 127. The USB address of the function containing the endpoint.
        ///
        /// > **Note**: The word "function" is synonymous with "device".
        function_address: u8,
        /// Value between 0 and 16. The index of the endpoint within the function.
        endpoint_number: u8,
        /// The `SETUP` packet to send first.
        setup_packet: [u8; 8],
        /// Length of the buffer that the device is allowed to write to.
        buffer_len: u16,
    },

    /// Emits a `SETUP` packet optionally followed with an outgoing `DATA` packet to an endpoint
    /// followed with an ingoing `DATA` packet of zero bytes.
    /// The endpoint must be of type [`EndpointTy::Control`].
    ///
    /// The endpoint will first have been reported with an [`Action::AllocateNewEndpoint`].
    ///
    /// > **Note**: This corresponds to a full host-to-device control transfer.
    EmitSetupOutInPacket {
        /// Identifier assigned by the [`UsbDevices`]. Must be passed back later when calling
        /// [`UsbDevices::out_packet_result`].
        id: PacketId,
        /// Value between 0 and 127. The USB address of the function containing the endpoint.
        ///
        /// > **Note**: The word "function" is synonymous with "device".
        function_address: u8,
        /// Value between 0 and 16. The index of the endpoint within the function.
        endpoint_number: u8,
        /// The `SETUP` packet to send first.
        setup_packet: [u8; 8],
        /// Data to be sent to the device in a `DATA` packet afterwards, or an empty `Vec` if
        /// no data packet has to be sent.
        data: Vec<u8>,
    },

    /// Start a wait. You must later call [`UsbDevices::wait_finished`].
    WaitStart {
        /// We use a [`PacketId`] to identify waits as well.
        id: PacketId,
        /// How long to wait.
        duration: Duration,
    },
}

impl UsbDevices {
    /// Initializes the state machine. Must be passed the number of ports on the root hub.
    ///
    /// All the ports are assumed to be either [`PortState::NotPowered`] or
    /// [`PortState::Disconnected`].
    pub fn new(root_hub_ports: NonZeroU8) -> Self {
        UsbDevices {
            allocated_default_endpoint: false,
            next_packet_id: PacketId(1),
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
    /// Panics if the port number is out of range compared to what was passed to
    /// [`UsbDevices::new`].
    ///
    /// Panics if the new state doesn't make sense when compared to the old state. In particular,
    /// the state machine of the [`UsbDevices`] assumes that it has exclusive control over the
    /// port (by generation [`Action::SetRootHubPortState`]). Shared ownership isn't (and can't
    /// be) supported.
    pub fn set_root_hub_port_state(&mut self, root_hub_port: NonZeroU8, new_state: PortState) {
        let mut state = &mut self.root_hub_ports[usize::from(root_hub_port.get() - 1)];
        match (&mut state, new_state) {
            // No update.
            (LocalPortState::Disconnected, PortState::NotPowered)
            | (LocalPortState::Disconnected, PortState::Disconnected)
            | (LocalPortState::Disabled, PortState::Disabled) => {}

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
            (from, to) => panic!("can't switch port state from {:?} to {:?}", from, to),
        }
    }

    /// Must be called as a response to [`Action::StartWait`].
    ///
    /// You are encouraged to call [`UsbDevices::next_action`] after this has returned.
    pub fn wait_finished(&mut self, id: PacketId) {
        assert!(id.0 < self.next_packet_id.0);

        for device in self.devices.values_mut() {
            match device.configuration_state {
                ConfigurationState::WaitingAddressResponsive { wait } if wait == Some(id) => {
                    device.configuration_state = ConfigurationState::EndpointNotAllocated;
                    return;
                }
                _ => {}
            }
        }

        panic!("unknown id in wait_finished()")
    }

    /// Must be called as a response to [`Action::EmitInPacket`], [`Action::EmitSetupInPacket`]
    /// or [`Action::EmitSetupOutInPacket`]. Contains the outcome of the packet.
    ///
    /// You are encouraged to call [`UsbDevices::next_action`] after this has returned.
    ///
    /// # Panic
    ///
    /// Panics if the [`PacketId`] is invalid or has already been responded to.
    // TODO: error type?
    pub fn in_packet_result(&mut self, id: PacketId, result: Result<&[u8], ()>) {
        assert!(id.0 < self.next_packet_id.0);

        // Check if this is a `SetAddress` packet.
        for port in &mut self.root_hub_ports {
            let addr = match port {
                LocalPortState::EnabledSendingAddress { packet_id, address }
                    if *packet_id == id =>
                {
                    *address
                }
                _ => continue,
            };

            assert!(result.is_ok()); // TODO: not implemented otherwise

            *port = LocalPortState::Address(addr);
            log::info!("assigned address {:?}", addr); // TODO: remove
            self.devices.insert(
                addr,
                Device {
                    configuration_state: ConfigurationState::WaitingAddressResponsive {
                        wait: None,
                    },
                    hub_address: None,
                },
            );
            return;
        }

        unimplemented!("in result")
    }

    /// Must be called as a response to [`Action::EmitOutPacket`]. Contains the outcome of the
    /// packet emission.
    ///
    /// You are encouraged to call [`UsbDevices::next_action`] after this has returned.
    ///
    /// # Panic
    ///
    /// Panics if the [`PacketId`] is invalid or has already been responded to.
    // TODO: error type?
    pub fn out_packet_result(&mut self, id: PacketId, result: Result<(), ()>) {
        assert!(id.0 < self.next_packet_id.0);

        unimplemented!("out result")
    }

    /// Asks the [`UsbDevices`] which action to perform next.
    ///
    /// You should call this method in a loop as long as it returns `Some`.
    pub fn next_action(&mut self) -> Option<Action> {
        // Allocating the default endpoint is a one-time event.
        if !self.allocated_default_endpoint {
            self.allocated_default_endpoint = true;
            return Some(Action::AllocateNewEndpoint {
                function_address: 0,
                endpoint_number: 0,
                ty: EndpointTy::Control,
            });
        }

        // Start resetting a port, if possible.
        if let Some(p) = self
            .root_hub_ports
            .iter()
            .position(|p| matches!(p, LocalPortState::Disabled))
        {
            // We only want to enable another port if no other port is currently resetting or on
            // the default address. No two ports must use the default address at a given time.
            let ready_to_enable = !self.root_hub_ports.iter().any(|p| {
                matches!(
                    p,
                    LocalPortState::Resetting | LocalPortState::EnabledDefaultAddress { .. }
                )
            });

            if ready_to_enable {
                self.root_hub_ports[p] = LocalPortState::Resetting;
                let port = NonZeroU8::new(u8::try_from(p + 1).unwrap()).unwrap();
                return Some(Action::SetRootHubPortState {
                    port,
                    state: PortState::Resetting,
                });
            }
        }

        // Send address packet to newly-enabled devices.
        if let Some(p) = self
            .root_hub_ports
            .iter()
            .position(|p| matches!(p, LocalPortState::EnabledDefaultAddress))
        {
            let packet_id = self.next_packet_id;
            self.next_packet_id.0 = self.next_packet_id.0.checked_add(1).unwrap();
            // TODO: proper address attribution
            let new_address = NonZeroU8::new(1).unwrap();
            self.root_hub_ports[p] = LocalPortState::EnabledSendingAddress {
                packet_id,
                address: new_address,
            };
            let (header, data) =
                control_packets::encode_request(control_packets::Request::set_address(new_address));
            let data = data.into_out_data().unwrap().to_vec();
            assert!(data.is_empty());
            // TODO: must give 2ms of rest for the device to listen on the new address
            return Some(Action::EmitSetupOutInPacket {
                id: packet_id,
                function_address: 0,
                endpoint_number: 0,
                setup_packet: header,
                data,
            });
        }

        for (function_address, device) in self.devices.iter_mut() {
            match device.configuration_state {
                ConfigurationState::EndpointNotAllocated => {
                    device.configuration_state = ConfigurationState::NotConfigured;
                    return Some(Action::AllocateNewEndpoint {
                        function_address: function_address.get(),
                        endpoint_number: 0,
                        ty: EndpointTy::Control,
                    });
                }

                ConfigurationState::NotConfigured => {
                    let packet_id = self.next_packet_id;
                    self.next_packet_id.0 = self.next_packet_id.0.checked_add(1).unwrap();
                    let (header, data) = control_packets::encode_request(
                        control_packets::Request::get_descriptor(0),
                    );
                    device.configuration_state =
                        ConfigurationState::DeviceDescriptorRequested(packet_id);
                    let buffer_len = data.into_in_buffer_len().unwrap();
                    return Some(Action::EmitSetupInPacket {
                        id: packet_id,
                        function_address: function_address.get(),
                        endpoint_number: 0,
                        setup_packet: header,
                        buffer_len,
                    });
                }

                ConfigurationState::WaitingAddressResponsive { wait: None } => {
                    let packet_id = self.next_packet_id;
                    self.next_packet_id.0 = self.next_packet_id.0.checked_add(1).unwrap();
                    device.configuration_state = ConfigurationState::WaitingAddressResponsive {
                        wait: Some(packet_id),
                    };
                    return Some(Action::WaitStart {
                        id: packet_id,
                        duration: Duration::from_millis(2),
                    });
                }

                _ => {}
            }
        }

        None
    }
}
