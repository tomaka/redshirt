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

//! Registering Ethernet interfaces.
//!
//! This module allows you to register your Ethernet interface. TCP and UDP sockets will then use
//! it to communicate with the outside.
//!
//! Use this if you're writing for example a networking driver or a VPN.
//!
//! # Usage
//!
//! - Call [`register_interface`] in order to notify of the existence of an interface.
//! - You obtain a [`NetInterfaceRegistration`] that you can use to report packets that came from
//! the wire, and from which you can obtain packets to send to the wire.
//! - Dropping the [`NetInterfaceRegistration`] unregisters the interface.
//!

use crate::ffi;
use core::fmt;
use futures::{lock::{Mutex, MutexGuard}, prelude::*};
use redshirt_syscalls::Encode as _;

/// Configuration of an interface to register.
#[derive(Debug)]
pub struct InterfaceConfig {
    /// MAC address of the interface.
    ///
    /// If this is a virtual device, feel free to randomly generate a MAC address.
    pub mac_address: [u8; 6],
}

/// Registers a new network interface.
pub async fn register_interface(config: InterfaceConfig) -> NetInterfaceRegistration {
    unsafe {
        let id = redshirt_random_interface::generate_u64().await;

        redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &{
            ffi::NetworkMessage::RegisterInterface {
                id,
                mac_address: config.mac_address,
            }
        })
        .unwrap();

        NetInterfaceRegistration {
            id,
            packet_from_net: Mutex::new(None),
            packet_to_net: Mutex::new((0..10).map(|_| build_packet_to_net(id)).collect()),
        }
    }
}

/// Registered network interface.
///
/// Destroying this object will unregister the interface.
pub struct NetInterfaceRegistration {
    /// Identifier of the interface in the network manager.
    id: u64,
    /// Future that will resolve once we receive a packet from the network manager to send to the
    /// network. Must always be `Some`.
    packet_to_net: Mutex<stream::FuturesUnordered<redshirt_syscalls::MessageResponseFuture<Vec<u8>>>>,
    /// Future that will resolve once we have successfully delivered a packet from the network,
    /// and are ready to deliver a next one.
    packet_from_net: Mutex<Option<redshirt_syscalls::MessageResponseFuture<()>>>,
}

/// Build a `Future` resolving to the next packet to send to the network.
///
/// Only one such `Future` must be alive at any given point in time.
fn build_packet_to_net(interface_id: u64) -> redshirt_syscalls::MessageResponseFuture<Vec<u8>> {
    unsafe {
        let message = ffi::NetworkMessage::InterfaceWaitData(interface_id).encode();
        let msg_id = redshirt_syscalls::MessageBuilder::new()
            .add_data(&message)
            .emit_with_response_raw(&ffi::INTERFACE)
            .unwrap();
        redshirt_syscalls::message_response(msg_id)
    }
}

impl NetInterfaceRegistration {
    /// Wait until the network manager is ready to accept a packet coming from the network.
    ///
    /// Returns a [`PacketFromNetwork`] object that allows you to transmit the packet.
    ///
    /// > **Note**: It is possible to call this method multiple times on the same
    /// >           [`NetInterfaceRegistration`]. If that is done, no guarantee exists as to which
    /// >           `Future` finishes first.
    pub async fn packet_from_network<'a>(&'a self) -> PacketFromNetwork<'a> {
        // Wait for the previous send to be finished.
        let mut packet_from_net = self.packet_from_net.lock().await;
        if let Some(fut) = packet_from_net.as_mut() {
            fut.await;
        }
        *packet_from_net = None;

        PacketFromNetwork {
            parent: self,
            send_future: packet_from_net,
        }
    }

    /// Returns the next packet to send to the network.
    ///
    /// > **Note**: It is possible to call this method multiple times on the same
    /// >           [`NetInterfaceRegistration`]. If that is done, no guarantee exists as to which
    /// >           `Future` finishes first.
    pub async fn packet_to_send(&self) -> Vec<u8> {
        let mut packet_to_net = self.packet_to_net.lock().await;
        let data = packet_to_net.next().await.unwrap();
        packet_to_net.push(build_packet_to_net(self.id));
        data
    }
}

impl fmt::Debug for NetInterfaceRegistration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("NetInterfaceRegistration")
            .field(&self.id)
            .finish()
    }
}

impl Drop for NetInterfaceRegistration {
    fn drop(&mut self) {
        unsafe {
            let message = ffi::NetworkMessage::UnregisterInterface(self.id);
            redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &message).unwrap();
        }
    }
}

/// Allows you to transmit a packet received from the network to the manager.
#[must_use]
pub struct PacketFromNetwork<'a> {
    parent: &'a NetInterfaceRegistration,
    send_future: MutexGuard<'a, Option<redshirt_syscalls::MessageResponseFuture<()>>>,
}

impl<'a> PacketFromNetwork<'a> {
    /// Send the packet to the manager.
    pub fn send(mut self, data: impl Into<Vec<u8>>) {
        unsafe {
            debug_assert!(self.send_future.is_none());
            let message =
                ffi::NetworkMessage::InterfaceOnData(self.parent.id, data.into()).encode();
            let msg_id = redshirt_syscalls::MessageBuilder::new()
                .add_data(&message)
                .emit_with_response_raw(&ffi::INTERFACE)
                .unwrap();
            let fut = redshirt_syscalls::message_response(msg_id);
            *self.send_future = Some(fut);
        }
    }
}
