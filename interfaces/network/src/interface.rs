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

//! Registering network interfaces.
//!
//! This module allows you to register your network interface. TCP and UDP sockets will then use
//! it to communicate with the outside.
//!
//! Use this if you're writing for example a networking driver or a VPN.
//!
//! # Usage
//!
//! - Call [`register_interface`] in order to notify the network manager of the existance of an
//! interface.
//! - You obtain a [`NetInterfaceRegistration`] that you can use to report packets that came from
//! the wire, and from which you can obtain packets to send to the wire.
//! - Dropping the [`NetInterfaceRegistration`] automatically unregisters the interface.
//!

use crate::ffi;
use core::{fmt, marker::PhantomData};
use futures::lock::{Mutex, MutexGuard};

/// Configuration of an interface to register.
#[derive(Debug)]
// TODO: #[non_exhaustive]
pub struct InterfaceConfig {
    /// MAC address of the interface.
    ///
    /// If this is a virtual device, feel free to randomly generate a MAC address.
    pub mac_address: [u8; 6],
}

/// Registers a new network interface.
pub fn register_interface(config: InterfaceConfig) -> NetInterfaceRegistration {
    unsafe {
        let id = 0xdeadbeef; // FIXME: generate randomly

        redshirt_syscalls_interface::emit_message_without_response(&ffi::INTERFACE, &{
            ffi::TcpMessage::RegisterInterface {
                id,
                mac_address: config.mac_address,
            }
        });

        NetInterfaceRegistration {
            id,
            packet_from_net: Mutex::new(None),
            packet_to_net: Mutex::new(build_packet_to_net(id)),
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
    packet_to_net: Mutex<redshirt_syscalls_interface::MessageResponseFuture<Vec<u8>>>,
    /// Future that will resolve once we have successfully delivered a packet from the network,
    /// and are ready to deliver a next one.
    packet_from_net: Mutex<Option<redshirt_syscalls_interface::MessageResponseFuture<()>>>,
}

/// Build a `Future` resolving to the next packet to send to the network.
///
/// Only one such `Future` must be alive at any given point in time.
fn build_packet_to_net(
    interface_id: u64,
) -> redshirt_syscalls_interface::MessageResponseFuture<Vec<u8>> {
    unsafe {
        let message = ffi::TcpMessage::InterfaceWaitData(interface_id);
        let msg_id = redshirt_syscalls_interface::emit_message(&ffi::INTERFACE, &message, true)
            .unwrap()
            .unwrap();
        redshirt_syscalls_interface::message_response(msg_id)
    }
}

// TODO: refactor API so that we don't use Mutexes internally?
// it is unfortunately quite hard to say what a convenient Futures API look like for now

impl NetInterfaceRegistration {
    /// Wait until the network manager is ready to accept a packet coming from the network.
    ///
    /// Returns a [`PacketFromNetwork`] object that allows you to transmit the packet.
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
    pub async fn packet_to_send(&self) -> Vec<u8> {
        let mut packet_to_net = self.packet_to_net.lock().await;
        let data = (&mut *packet_to_net).await;
        *packet_to_net = build_packet_to_net(self.id);
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
            let message = ffi::TcpMessage::UnregisterInterface(self.id);
            redshirt_syscalls_interface::emit_message_without_response(&ffi::INTERFACE, &message);
        }
    }
}

/// Allows you to transmit a packet received from the network to the manager.
#[must_use]
pub struct PacketFromNetwork<'a> {
    parent: &'a NetInterfaceRegistration,
    send_future: MutexGuard<'a, Option<redshirt_syscalls_interface::MessageResponseFuture<()>>>,
}

impl<'a> PacketFromNetwork<'a> {
    /// Send the packet to the manager.
    pub fn send(mut self, data: impl Into<Vec<u8>>) {
        unsafe {
            debug_assert!(self.send_future.is_none());
            let message = ffi::TcpMessage::InterfaceOnData(self.parent.id, data.into());
            let msg_id = redshirt_syscalls_interface::emit_message(&ffi::INTERFACE, &message, true)
                .unwrap()
                .unwrap();
            let fut = redshirt_syscalls_interface::message_response(msg_id);
            *self.send_future = Some(fut);
        }
    }
}
