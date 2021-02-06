// Copyright (C) 2019-2021  Pierre Krieger
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

//! Manages a collection of network interfaces.
//!
//! This module manages the state of all the network interfaces together. Most of the
//! implementation is delegated to the [`interface`] module, and the primary role of this code
//! is to aggregate interfaces and assign new sockets to the correct interface based on the
//! available routes.

use crate::interface;

use fnv::FnvBuildHasher;
use futures::prelude::*;
use hashbrown::{hash_map::Entry, HashMap};
use std::{fmt, hash::Hash, iter, mem, net::SocketAddr, pin::Pin};

/// State machine managing all the network interfaces and sockets.
///
/// The `TIfId` generic parameter is an identifier for network interfaces.
/// The `TSockUd` generic parameter is user data to store alongside with each socket.
pub struct NetworkManager<TIfId, TIfUser, TSockUd> {
    /// List of devices that have been registered.
    devices: HashMap<TIfId, Device<TIfUser, TSockUd>, FnvBuildHasher>,
    /// Id to assign to the next socket.
    next_socket_id: u64,
    /// List of sockets open in the manager.
    sockets: HashMap<u64, SocketState<TIfId, TSockUd>, FnvBuildHasher>,
}

/// State of a socket.
#[derive(Debug)]
enum SocketState<TIfId, TSockUd> {
    /// Socket is waiting to be assigned to an interface.
    Pending {
        /// `listen` parameter passed to the socket constructor.
        listen: bool,
        /// Socket address parameter passed to the socket constructor.
        addr: SocketAddr,
        /// User data for this socket.
        user_data: TSockUd,
    },
    /// Socket has been assigned to a specific interface.
    Assigned {
        /// Interface it's been assigned to.
        interface: TIfId,
        /// Id of the socket within the interface.
        inner_id: interface::SocketId,
    },
}

/// State of a device.
struct Device<TIfUser, TSockUd> {
    /// Inner state.
    inner: interface::NetInterfaceState<(u64, TSockUd)>,
    /// Additional user data.
    user_data: TIfUser,
}

/// Event generated by the [`NetworkManager::next_event`] function.
#[derive(Debug)]
pub enum NetworkManagerEvent<'a, TIfId, TIfUser, TSockUd> {
    /// Data to be sent out by the Ethernet cable is available.
    EthernetCableOut(Interface<'a, TIfId, TIfUser, TSockUd>),
    /// A TCP/IP socket has connected to its target.
    TcpConnected {
        socket: TcpSocket<'a, TIfId, TIfUser, TSockUd>,
        local_endpoint: SocketAddr,
        remote_endpoint: SocketAddr,
    },
    /// A TCP/IP socket has been closed by the remote.
    ///
    /// > **Note**: This does *not* destroy the socket. You must call [`TcpSocket::reset`] to
    /// >           actually destroy it.
    TcpClosed(TcpSocket<'a, TIfId, TIfUser, TSockUd>),
    /// A TCP/IP socket has data ready to be read.
    TcpReadReady(TcpSocket<'a, TIfId, TIfUser, TSockUd>),
    /// A TCP/IP socket has finished writing the data that we passed to it, and is now ready to
    /// accept more.
    TcpWriteFinished(TcpSocket<'a, TIfId, TIfUser, TSockUd>),
}

/// Internal enum similar to [`NetworkManagerEvent`], except that it is `'static`.
///
/// Necessary because of borrow checker issue.
// TODO: remove this once Polonius lands in Rust
#[derive(Debug)]
enum NetworkManagerEventStatic {
    EthernetCableOut,
    TcpConnected(interface::SocketId, SocketAddr, SocketAddr),
    TcpClosed(interface::SocketId),
    TcpReadReady(interface::SocketId),
    TcpWriteFinished(interface::SocketId),
    DhcpDiscovery,
}

/// Identifier of a socket within the [`NetworkManager`]. Common between all types of sockets.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct SocketId {
    id: u64,
}

impl<TIfId, TIfUser, TSockUd> NetworkManager<TIfId, TIfUser, TSockUd>
where
    TIfId: Clone + Hash + PartialEq + Eq,
{
    /// Initializes a new `NetworkManager`.
    pub fn new() -> Self {
        NetworkManager {
            devices: HashMap::default(),
            next_socket_id: 1,
            sockets: HashMap::default(),
        }
    }

    /// Adds a new TCP socket to the state of the network manager.
    ///
    /// If `listen` is `true`, then `addr` is a local address that the socket will listen on.
    pub fn build_tcp_socket(
        &mut self,
        listen: bool,
        addr: &SocketAddr,
        user_data: TSockUd,
    ) -> TcpSocket<TIfId, TIfUser, TSockUd> {
        let socket_id = self.next_socket_id;
        self.next_socket_id += 1;

        let mut user_data = Some(user_data);

        for (device_id, device) in self.devices.iter_mut() {
            // TODO: naive
            match device.inner.build_tcp_socket(
                listen,
                addr,
                (socket_id, user_data.take().unwrap()),
            ) {
                Ok(socket) => {
                    self.sockets.insert(
                        socket_id,
                        SocketState::Assigned {
                            interface: device_id.clone(),
                            inner_id: socket.id(),
                        },
                    );

                    return TcpSocket {
                        parent: self,
                        id: socket_id,
                    };
                }
                Err((_, (_, ud))) => {
                    user_data = Some(ud);
                }
            }
        }

        self.sockets.insert(
            socket_id,
            SocketState::Pending {
                listen,
                user_data: user_data.take().unwrap(),
                addr: addr.clone(),
            },
        );

        TcpSocket {
            parent: self,
            id: socket_id,
        }
    }

    /// Returns an accesss to the TCP socket with the given id.
    pub fn tcp_socket_by_id(
        &mut self,
        id: &SocketId,
    ) -> Option<TcpSocket<TIfId, TIfUser, TSockUd>> {
        if !self.sockets.contains_key(&id.id) {
            return None;
        }

        Some(TcpSocket {
            parent: self,
            id: id.id,
        })
    }

    /// Registers an interface with the given ID. Returns an error if an interface with that ID
    /// already exists.
    pub async fn register_interface<'a>(
        &'a mut self,
        id: TIfId,
        mac_address: [u8; 6],
        user_data: TIfUser,
    ) -> Result<Interface<'a, TIfId, TIfUser, TSockUd>, ()> {
        let entry = match self.devices.entry(id.clone()) {
            Entry::Occupied(_) => return Err(()),
            Entry::Vacant(e) => e,
        };

        log::debug!(
            "Registering interface with MAC {:>02X}:{:>02X}:{:>02X}:{:>02X}:{:>02X}:{:>02X}",
            mac_address[0],
            mac_address[1],
            mac_address[2],
            mac_address[3],
            mac_address[4],
            mac_address[5]
        );

        let interface = interface::NetInterfaceState::new(interface::Config {
            ip_address: interface::ConfigIpAddr::DHCPv4,
            mac_address,
        })
        .await;

        entry.insert(Device {
            inner: interface,
            user_data,
        });

        Ok(Interface { parent: self, id })
    }

    /// Returns an accesss to the interface with the given id.
    pub fn interface_by_id(&mut self, id: TIfId) -> Option<Interface<TIfId, TIfUser, TSockUd>> {
        if !self.devices.contains_key(&id) {
            return None;
        }

        Some(Interface { parent: self, id })
    }

    /// Returns the next event generated by the [`NetworkManager`].
    pub async fn next_event<'a>(&'a mut self) -> NetworkManagerEvent<'a, TIfId, TIfUser, TSockUd> {
        loop {
            let (device_id, event) = self.next_event_inner().await;
            match event {
                NetworkManagerEventStatic::EthernetCableOut => {
                    let device = self.devices.get_mut(&device_id).unwrap();
                    return NetworkManagerEvent::EthernetCableOut(Interface {
                        parent: self,
                        id: device_id,
                    });
                }
                NetworkManagerEventStatic::TcpConnected(
                    socket,
                    local_endpoint,
                    remote_endpoint,
                ) => {
                    let device = self.devices.get_mut(&device_id).unwrap();
                    let inner = device.inner.tcp_socket_by_id(socket).unwrap();
                    let id = inner.user_data().0;
                    return NetworkManagerEvent::TcpConnected {
                        socket: TcpSocket { parent: self, id },
                        local_endpoint,
                        remote_endpoint,
                    };
                }
                NetworkManagerEventStatic::TcpClosed(socket) => {
                    let device = self.devices.get_mut(&device_id).unwrap();
                    let inner = device.inner.tcp_socket_by_id(socket).unwrap();
                    let id = inner.user_data().0;
                    return NetworkManagerEvent::TcpClosed(TcpSocket { parent: self, id });
                }
                NetworkManagerEventStatic::TcpReadReady(socket) => {
                    let device = self.devices.get_mut(&device_id).unwrap();
                    let inner = device.inner.tcp_socket_by_id(socket).unwrap();
                    let id = inner.user_data().0;
                    return NetworkManagerEvent::TcpReadReady(TcpSocket { parent: self, id });
                }
                NetworkManagerEventStatic::TcpWriteFinished(socket) => {
                    let device = self.devices.get_mut(&device_id).unwrap();
                    let inner = device.inner.tcp_socket_by_id(socket).unwrap();
                    let id = inner.user_data().0;
                    return NetworkManagerEvent::TcpWriteFinished(TcpSocket { parent: self, id });
                }
                NetworkManagerEventStatic::DhcpDiscovery => {
                    let interface = self.devices.get_mut(&device_id).unwrap();

                    // Take all the pending sockets and try to assign them to that new interface.
                    // TODO: that's O(n)
                    let sockets = {
                        let cap = self.sockets.capacity();
                        mem::replace(
                            &mut self.sockets,
                            HashMap::with_capacity_and_hasher(cap, Default::default()),
                        )
                    };

                    for (socket_id, socket) in sockets {
                        let (listen, addr, user_data) = match socket {
                            SocketState::Pending {
                                listen,
                                addr,
                                user_data,
                            } => (listen, addr, user_data),
                            s @ SocketState::Assigned { .. } => {
                                self.sockets.insert(socket_id, s);
                                continue;
                            }
                        };

                        // TODO: naive
                        match interface.inner.build_tcp_socket(
                            listen,
                            &addr,
                            (socket_id, user_data),
                        ) {
                            Ok(inner_socket) => {
                                self.sockets.insert(
                                    socket_id,
                                    SocketState::Assigned {
                                        interface: device_id.clone(),
                                        inner_id: inner_socket.id(),
                                    },
                                );
                            }
                            Err((_, (_, user_data))) => {
                                self.sockets.insert(
                                    socket_id,
                                    SocketState::Pending {
                                        listen,
                                        addr,
                                        user_data,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    async fn next_event_inner<'a>(&'a mut self) -> (TIfId, NetworkManagerEventStatic) {
        // TODO: optimize?
        let next_event = future::select_all(
            self.devices
                .iter_mut()
                .map(move |(n, d)| {
                    let user_data = &mut d.user_data;
                    Box::pin(
                        d.inner
                            .next_event()
                            .map(move |ev| (n.clone(), user_data, ev)),
                    ) as Pin<Box<dyn Future<Output = _>>>
                })
                .chain(iter::once(Box::pin(future::pending()) as Pin<Box<_>>)),
        );

        match next_event.await.0 {
            (device_id, _, interface::NetInterfaceEvent::EthernetCableOut) => {
                (device_id, NetworkManagerEventStatic::EthernetCableOut)
            }
            (
                device_id,
                _,
                interface::NetInterfaceEvent::TcpConnected {
                    socket,
                    local_endpoint,
                    remote_endpoint,
                },
            ) => (
                device_id,
                NetworkManagerEventStatic::TcpConnected(
                    socket.id(),
                    local_endpoint,
                    remote_endpoint,
                ),
            ),
            (device_id, _, interface::NetInterfaceEvent::TcpClosed(inner)) => {
                (device_id, NetworkManagerEventStatic::TcpClosed(inner.id()))
            }
            (device_id, _, interface::NetInterfaceEvent::TcpReadReady(inner)) => (
                device_id,
                NetworkManagerEventStatic::TcpReadReady(inner.id()),
            ),
            (device_id, _, interface::NetInterfaceEvent::TcpWriteFinished(inner)) => (
                device_id,
                NetworkManagerEventStatic::TcpWriteFinished(inner.id()),
            ),
            (device_id, _, interface::NetInterfaceEvent::DhcpDiscovery { .. }) => {
                (device_id, NetworkManagerEventStatic::DhcpDiscovery)
            }
        }
    }
}

impl<'a, TIfId, TIfUser, TSockUd> fmt::Debug for NetworkManager<TIfId, TIfUser, TSockUd> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: better impl?
        f.debug_tuple("NetworkManager").finish()
    }
}

/// Access to an interface within the manager.
pub struct Interface<'a, TIfId, TIfUser, TSockUd> {
    parent: &'a mut NetworkManager<TIfId, TIfUser, TSockUd>,
    id: TIfId,
}

impl<'a, TIfId, TIfUser, TSockUd> Interface<'a, TIfId, TIfUser, TSockUd>
where
    TIfId: Clone + Hash + PartialEq + Eq,
{
    pub fn unregister(self) {
        //let device = self.devices.remove(id);
        todo!();
        // TODO: this is far from trivial, as one has to kill all sockets
    }

    /// Extract the data to transmit out of the Ethernet cable.
    ///
    /// Returns an empty buffer if nothing is ready.
    pub fn user_data(&mut self) -> &mut TIfUser {
        &mut self.parent.devices.get_mut(&self.id).unwrap().user_data
    }

    /// Extract the data to transmit out of the Ethernet cable.
    ///
    /// Returns an empty buffer if nothing is ready.
    pub fn read_ethernet_cable_out(&mut self) -> Vec<u8> {
        self.parent
            .devices
            .get_mut(&self.id)
            .unwrap()
            .inner
            .read_ethernet_cable_out()
    }

    /// Injects some data coming from the Ethernet cable.
    pub fn inject_data(&mut self, data: impl AsRef<[u8]>) {
        self.parent
            .devices
            .get_mut(&self.id)
            .unwrap()
            .inner
            .inject_interface_data(data)
    }
}

impl<'a, TIfId, TIfUser, TSockUd> fmt::Debug for Interface<'a, TIfId, TIfUser, TSockUd>
where
    TIfId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: better impl
        f.debug_tuple("Interface").finish()
    }
}

/// Access to a socket within the manager.
pub struct TcpSocket<'a, TIfId, TIfUser, TSockUd> {
    parent: &'a mut NetworkManager<TIfId, TIfUser, TSockUd>,
    id: u64,
}

impl<'a, TIfId, TIfUser, TSockUd> TcpSocket<'a, TIfId, TIfUser, TSockUd>
where
    TIfId: Clone + Hash + PartialEq + Eq,
{
    /// Returns the identifier of the socket, for later retrieval.
    pub fn id(&self) -> SocketId {
        SocketId { id: self.id }
    }

    /// Returns a reference to the user data stored within this TCP socket.
    pub fn user_data_mut(&mut self) -> &mut TSockUd {
        match self.parent.sockets.get_mut(&self.id).unwrap() {
            SocketState::Pending { user_data, .. } => user_data,
            SocketState::Assigned {
                interface,
                inner_id,
            } => {
                &mut self
                    .parent
                    .devices
                    .get_mut(interface)
                    .unwrap()
                    .inner
                    .tcp_socket_by_id(*inner_id)
                    .unwrap()
                    .into_user_data()
                    .1
            }
        }
    }

    /// Reads the data that has been received on the TCP socket.
    ///
    /// Returns an empty `Vec` if there is no data available.
    ///
    /// # Panic
    ///
    /// Panics if the socket is still in the connecting stage.
    pub fn read(&mut self) -> Vec<u8> {
        match self.parent.sockets.get_mut(&self.id).unwrap() {
            SocketState::Pending { .. } => panic!(),
            SocketState::Assigned {
                interface,
                inner_id,
            } => self
                .parent
                .devices
                .get_mut(interface)
                .unwrap()
                .inner
                .tcp_socket_by_id(*inner_id)
                .unwrap()
                .read(),
        }
    }

    /// Passes a buffer that the socket will encode into Ethernet frames.
    ///
    /// Only one buffer can be active at any given point in time. If a buffer is already active,
    /// returns `Err(buffer)`.
    ///
    /// # Panic
    ///
    /// Panics if the socket is still in the connecting stage.
    pub fn set_write_buffer(&mut self, buffer: Vec<u8>) -> Result<(), Vec<u8>> {
        match self.parent.sockets.get_mut(&self.id).unwrap() {
            SocketState::Pending { .. } => panic!(),
            SocketState::Assigned {
                interface,
                inner_id,
            } => self
                .parent
                .devices
                .get_mut(interface)
                .unwrap()
                .inner
                .tcp_socket_by_id(*inner_id)
                .unwrap()
                .set_write_buffer(buffer),
        }
    }

    /// Starts the process of closing the TCP socket.
    ///
    /// Returns an error if `closed` had been called earlier on this socket. This error is benign.
    ///
    /// # Panic
    ///
    /// Panics if the socket is still in the connecting stage.
    pub fn close(&mut self) -> Result<(), ()> {
        match self.parent.sockets.get_mut(&self.id).unwrap() {
            SocketState::Pending { .. } => panic!(),
            SocketState::Assigned {
                interface,
                inner_id,
            } => self
                .parent
                .devices
                .get_mut(interface)
                .unwrap()
                .inner
                .tcp_socket_by_id(*inner_id)
                .unwrap()
                .close(),
        }
    }

    /// Returns true if `close` has successfully been called earlier.
    pub fn close_called(&mut self) -> bool {
        match self.parent.sockets.get(&self.id).unwrap() {
            SocketState::Pending { .. } => false,
            SocketState::Assigned {
                interface,
                inner_id,
            } => self
                .parent
                .devices
                .get_mut(interface)
                .unwrap()
                .inner
                .tcp_socket_by_id(*inner_id)
                .unwrap()
                .close_called(),
        }
    }

    /// Returns true if the socket has been closed.
    ///
    /// > **Note**: This indicates whether the socket is entirely closed, including by the remote,
    /// >           and isn't directly related to the `close` method.
    pub fn closed(&mut self) -> bool {
        match self.parent.sockets.get(&self.id).unwrap() {
            SocketState::Pending { .. } => false,
            SocketState::Assigned {
                interface,
                inner_id,
            } => self
                .parent
                .devices
                .get_mut(interface)
                .unwrap()
                .inner
                .tcp_socket_by_id(*inner_id)
                .unwrap()
                .closed(),
        }
    }

    /// Destroys the socket. If it was open, instantly drops everything.
    pub fn reset(self) {
        match self.parent.sockets.remove(&self.id).unwrap() {
            SocketState::Pending { .. } => {}
            SocketState::Assigned {
                interface,
                inner_id,
            } => self
                .parent
                .devices
                .get_mut(&interface)
                .unwrap()
                .inner
                .tcp_socket_by_id(inner_id)
                .unwrap()
                .reset(),
        }
    }
}

impl<'a, TIfId, TIfUser, TSockUd> fmt::Debug for TcpSocket<'a, TIfId, TIfUser, TSockUd>
where
    TIfId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: better impl
        f.debug_tuple("TcpSocket").finish()
    }
}