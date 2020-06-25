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

//! Manages a registered networking interface.
//!
//! This module manages the state of a single networking interface. This state consists of:
//!
//! - The local MAC address.
//! - The local IP address, sub-net mask, and gateway.
//! - The known neighbouring nodes, automatically discovered through ARP or NDP.
//! - A list of TCP sockets active on this interface and their state.
//! - A buffer of data waiting to be sent out on the interface. It is the role of the user of this
//! module to empty this buffer.
//! - (Optional) The state of a DHCP client.
//!
//! > **Note**: Most of this is delegated to the `smoltcp` library, but this should be considered
//! >           as an implementation detail.
//!
//! # Usage
//!
//! - Create a [`NetInterfaceState`] by calling [`NetInterfaceState::new`].
//! - When some data arrives from the network, call [`NetInterfaceState::inject_interface_data`].
//! - Call [`NetInterfaceState::next_event`] to be informed of events on the interface. Events can
//! be generated in response to a call to [`NetInterfaceState::inject_interface_data`], but also
//! spontaneously after a certain time has elapsed.
//! - If [`NetInterfaceEvent::EthernetCableOut`] is generated, call
//! [`NetInterfaceState::read_ethernet_cable_out`] in order to obtain the data to send out to the
//! network.
//!

// TODO: write more docs ^
// TODO: implement UDP

use crate::port_assign;

use fnv::FnvBuildHasher;
use hashbrown::HashMap;
use smoltcp::{dhcp::Dhcpv4Client, phy, time::Instant};
use std::{
    cmp,
    collections::BTreeMap,
    convert::TryFrom as _,
    fmt, mem,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::MutexGuard,
    time::Duration,
};

/// State machine encompassing an Ethernet interface and the sockets operating on it.
// TODO: Debug
pub struct NetInterfaceState<TSockUd> {
    /// State of the Ethernet interface.
    ethernet: smoltcp::iface::EthernetInterface<'static, 'static, 'static, RawDevice>,

    /// State of the DHCPv4 client, if enabled.
    dhcp_v4_client: Option<Dhcpv4Client>,

    /// If false, we have to report to the user that data is available (if that is the case).
    reported_available_data: bool,

    /// Collection of all the active sockets that currently operate on this interface.
    sockets: smoltcp::socket::SocketSet<'static, 'static, 'static>,

    /// State of the sockets. Maintained in parallel with [`NetInterfaceState`].
    sockets_state: HashMap<SocketId, SocketState<TSockUd>, FnvBuildHasher>,

    /// TCP ports reservation.
    tcp_ports_assign: port_assign::PortAssign,

    /// If true, we should check the state of all the sockets at the next call to `next_event`.
    check_sockets_required: bool,

    /// Future that triggers the next time we should poll [`NetInterfaceState::ethernet`].
    /// Must be set to `None` whenever we modify [`NetInterfaceState::ethernet`] in such a way that
    /// it could produce an event.
    ethernet_poll_delay: Option<redshirt_time_interface::Delay>,
}

/// Configuration for a [`NetInterfaceState`] under construction.
#[derive(Debug)]
pub struct Config {
    /// How the interface knows its own IP address.
    pub ip_address: ConfigIpAddr,
    /// MAC address of the device.
    pub mac_address: [u8; 6],
}

/// How the interface knows its IP address.
#[derive(Debug)]
pub enum ConfigIpAddr {
    /// IP address defined ahead of time.
    FixedIpv4 {
        ip_address: Ipv4Addr,
        /// Length in bits of the subnet mask.
        /// Must be inferior to 32.
        prefix_len: u8,
        gateway: Ipv4Addr,
    },

    /// IP address defined ahead of time.
    FixedIpv6 {
        ip_address: Ipv6Addr,
        /// Length in bits of the subnet mask.
        /// Must be inferior to 128.
        prefix_len: u8,
        gateway: Ipv6Addr,
    },

    /// Use DHCPv4 to automatically discover the surrounding IPv4 network.
    ///
    /// A [`NetInterfaceEvent::DhcpDiscovery`] event will be generated on success.
    DHCPv4,
}

/// Event generated by the [`NetInterfaceState::next_event`] function.
#[derive(Debug)]
pub enum NetInterfaceEvent<'a, TSockUd> {
    /// Data is available to be sent out by the Ethernet cable.
    EthernetCableOut,
    /// A TCP/IP socket has connected to its target.
    TcpConnected {
        socket: TcpSocket<'a, TSockUd>,
        local_endpoint: SocketAddr,
        remote_endpoint: SocketAddr,
    },
    /// A TCP/IP socket has been closed by the remote.
    TcpClosed(TcpSocket<'a, TSockUd>),
    /// A TCP/IP socket has data ready to be read.
    TcpReadReady(TcpSocket<'a, TSockUd>),
    /// A TCP/IP socket has finished writing the data that we passed to it, and is now ready to
    /// accept more.
    TcpWriteFinished(TcpSocket<'a, TSockUd>),

    /// The DHCP client has configured the interface.
    DhcpDiscovery {
        /// IP assigned to the interface.
        ip: Ipv4Addr,
        /// Length in bits of the subnet mask. Always inferior or equal to 32.
        prefix_len: u8,
        /// Default gateway when sending requests outside of the subnet mask.
        gateway: Ipv4Addr,
        /// Addresses of DNS servers reported by the DHCP server.
        dns_servers: Vec<Ipv4Addr>,
    },
}

/// Internal enum similar to [`NetInterfaceEvent`], except that it is `'static`.
///
/// Necessary because of borrow checker issue.
// TODO: remove this once Polonius lands in Rust
#[derive(Debug)]
enum NetInterfaceEventStatic {
    EthernetCableOut,
    TcpConnected(SocketId, SocketAddr, SocketAddr),
    TcpClosed(SocketId),
    TcpReadReady(SocketId),
    TcpWriteFinished(SocketId),
    DhcpDiscovery {
        ip: Ipv4Addr,
        prefix_len: u8,
        gateway: Ipv4Addr,
        dns_servers: Vec<Ipv4Addr>,
    },
}

/// Active TCP socket within a [`NetInterfaceState`].
pub struct TcpSocket<'a, TSockUd> {
    /// Reference to the interface.
    interface: &'a mut NetInterfaceState<TSockUd>,
    /// Identifier of that socket within [`NetInterfaceState::sockets`].
    id: SocketId,
}

/// State of a socket that we maintain in parallel to its actual state.
struct SocketState<TSockUd> {
    user_data: TSockUd,
    is_connected: bool,
    is_closed: bool,
    read_ready: bool,
    write_ready: bool,
    write_remaining: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("No port available")]
    NoPortAvailable,
    #[error("The specific port requested isn't available")]
    PortNotAvailable,
    #[error("The destination IP cannot be 0.0.0.0 or [::]")]
    UnspecifiedDestinationIp,
    #[error("The destination port cannot be 0")]
    UnspecifiedDestinationPort,
}

/// Opaque identifier of a socket within a [`NetInterfaceState`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketId(smoltcp::socket::SocketHandle);

impl<TSockUd> NetInterfaceState<TSockUd> {
    pub async fn new(config: Config) -> Self {
        let device = RawDevice {
            device_out_buffer: Vec::new(),
            device_in_buffer: Vec::with_capacity(4096),
        };

        let mut routes = smoltcp::iface::Routes::new(BTreeMap::new());
        let mut ip_addresses = Vec::new();

        match config.ip_address {
            ConfigIpAddr::FixedIpv4 {
                ip_address,
                prefix_len,
                gateway,
            } => {
                routes.add_default_ipv4_route(From::from(gateway));
                assert!(prefix_len <= 32);
                ip_addresses.push(From::from(smoltcp::wire::Ipv4Cidr::new(
                    From::from(ip_address),
                    prefix_len,
                )));
            }
            ConfigIpAddr::FixedIpv6 {
                ip_address,
                prefix_len,
                gateway,
            } => {
                routes.add_default_ipv6_route(From::from(gateway));
                assert!(prefix_len <= 128);
                ip_addresses.push(From::from(smoltcp::wire::Ipv6Cidr::new(
                    From::from(ip_address),
                    prefix_len,
                )));
            }
            ConfigIpAddr::DHCPv4 => {
                // We need to "reserve" one unspecified IP address, as specified in the
                // documentation of the DHCP client. It is unclear whether this is a strict
                // requirement or an optimization.
                ip_addresses.push(From::from(smoltcp::wire::Ipv4Cidr::new(
                    smoltcp::wire::Ipv4Address::UNSPECIFIED,
                    0,
                )));
            }
        }

        let interface = smoltcp::iface::EthernetInterfaceBuilder::new(device)
            .ethernet_addr(smoltcp::wire::EthernetAddress(config.mac_address))
            .ip_addrs(ip_addresses)
            .routes(routes)
            .neighbor_cache(smoltcp::iface::NeighborCache::new(BTreeMap::new()))
            .finalize();

        let mut sockets = smoltcp::socket::SocketSet::new(Vec::new());

        // Build the DHCP client, if relevant.
        // This inserts an entry in `sockets`.
        let dhcp_v4_client = if matches!(config.ip_address, ConfigIpAddr::DHCPv4) {
            let dhcp_rx_buffer = smoltcp::socket::RawSocketBuffer::new(
                [smoltcp::socket::RawPacketMetadata::EMPTY; 1],
                vec![0; 600],
            );

            let dhcp_tx_buffer = smoltcp::socket::RawSocketBuffer::new(
                [smoltcp::socket::RawPacketMetadata::EMPTY; 1],
                vec![0; 600],
            );

            Some(Dhcpv4Client::new(
                &mut sockets,
                dhcp_rx_buffer,
                dhcp_tx_buffer,
                now().await,
            ))
        } else {
            None
        };

        NetInterfaceState {
            ethernet: interface,
            reported_available_data: false,
            sockets,
            sockets_state: HashMap::default(),
            tcp_ports_assign: port_assign::PortAssign::new(),
            check_sockets_required: false,
            ethernet_poll_delay: None,
            dhcp_v4_client,
        }
    }

    /// Returns the IP address and prefix of the interface, or `None` if DHCP hasn't configured
    /// the interface yet.
    pub fn local_ip_prefix(&self) -> Option<(IpAddr, u8)> {
        assert_eq!(self.ethernet.ip_addrs().len(), 1);
        let addr = &self.ethernet.ip_addrs()[0];

        let ip = match addr.address() {
            smoltcp::wire::IpAddress::Unspecified => return None,
            smoltcp::wire::IpAddress::Ipv4(ip) => IpAddr::from(Ipv4Addr::from(ip)),
            smoltcp::wire::IpAddress::Ipv6(ip) => IpAddr::from(Ipv6Addr::from(ip)),
            _ => unimplemented!(),
        };

        let prefix = addr.prefix_len();
        Some((ip, prefix))
    }

    /// Initializes a new TCP connection which tries to connect to the given
    /// [`SocketAddr`](std::net::SocketAddr).
    pub fn build_tcp_socket(
        &mut self,
        listen: bool,
        addr: &SocketAddr,
        user_data: TSockUd,
    ) -> Result<TcpSocket<TSockUd>, (ConnectError, TSockUd)> {
        let mut socket = {
            let rx_buf = smoltcp::socket::TcpSocketBuffer::new(vec![0; 1024]);
            let tx_buf = smoltcp::socket::TcpSocketBuffer::new(vec![0; 1024]);
            smoltcp::socket::TcpSocket::new(rx_buf, tx_buf)
        };

        if listen {
            let mut addr = addr.clone();
            assert!(!addr.ip().is_multicast()); // TODO: ?
            if addr.port() == 0 {
                addr.set_port(match self.tcp_ports_assign.reserve_any(1024) {
                    Some(p) => p,
                    None => return Err((ConnectError::NoPortAvailable, user_data)),
                });
            } else {
                if let Err(()) = self.tcp_ports_assign.reserve(addr.port()) {
                    return Err((ConnectError::PortNotAvailable, user_data));
                }
            }
            // `listen` can only fail if the socket was misconfigured.
            socket.listen(addr).unwrap();
        } else {
            if addr.port() == 0 {
                return Err((ConnectError::UnspecifiedDestinationPort, user_data));
            }
            if addr.ip().is_unspecified() {
                return Err((ConnectError::UnspecifiedDestinationIp, user_data));
            }
            assert!(!addr.ip().is_multicast()); // TODO: not supported? or is it?
            let port = match self.tcp_ports_assign.reserve_any(1024) {
                Some(p) => p,
                None => return Err((ConnectError::NoPortAvailable, user_data)),
            };
            // `connect` can only fail if the socket was misconfigured.
            socket.connect(addr.clone(), port).unwrap();
        }

        let id = SocketId(self.sockets.add(socket));
        self.sockets_state.insert(
            id,
            SocketState {
                user_data,
                is_connected: false,
                is_closed: false,
                read_ready: false,
                write_ready: true,
                write_remaining: Vec::new(),
            },
        );
        self.ethernet_poll_delay = None;

        Ok(TcpSocket {
            interface: self,
            id,
        })
    }

    /// Returns an existing TCP socket by its ID.
    pub fn tcp_socket_by_id(&mut self, id: SocketId) -> Option<TcpSocket<TSockUd>> {
        if !self.sockets_state.contains_key(&id) {
            return None;
        }

        Some(TcpSocket {
            interface: self,
            id,
        })
    }

    /// Extract the data to transmit out of the Ethernet cable.
    ///
    /// Returns an empty buffer if nothing is ready.
    pub fn read_ethernet_cable_out(&mut self) -> Vec<u8> {
        let mut device_out_buffer = &mut self.ethernet.device_mut().device_out_buffer;
        self.reported_available_data = false;
        mem::replace(&mut *device_out_buffer, Vec::new())
    }

    /// Injects some data coming from the Ethernet cable.
    ///
    /// Call [`NetInterfaceState::next_event`] in order to obtain the result.
    pub fn inject_interface_data(&mut self, data: impl AsRef<[u8]>) {
        self.ethernet
            .device_mut()
            .device_in_buffer
            .extend_from_slice(data.as_ref());
        self.ethernet_poll_delay = None;
    }

    /// Wait until an event happens on the network.
    pub async fn next_event<'a>(&'a mut self) -> NetInterfaceEvent<'a, TSockUd> {
        match self.next_event_static().await {
            NetInterfaceEventStatic::EthernetCableOut => NetInterfaceEvent::EthernetCableOut,
            NetInterfaceEventStatic::TcpConnected(id, local_endpoint, remote_endpoint) => {
                NetInterfaceEvent::TcpConnected {
                    socket: self.tcp_socket_by_id(id).unwrap(),
                    local_endpoint,
                    remote_endpoint,
                }
            }
            NetInterfaceEventStatic::TcpClosed(id) => {
                NetInterfaceEvent::TcpClosed(self.tcp_socket_by_id(id).unwrap())
            }
            NetInterfaceEventStatic::TcpReadReady(id) => {
                NetInterfaceEvent::TcpReadReady(self.tcp_socket_by_id(id).unwrap())
            }
            NetInterfaceEventStatic::TcpWriteFinished(id) => {
                NetInterfaceEvent::TcpWriteFinished(self.tcp_socket_by_id(id).unwrap())
            }
            NetInterfaceEventStatic::DhcpDiscovery {
                ip,
                prefix_len,
                gateway,
                dns_servers,
            } => NetInterfaceEvent::DhcpDiscovery {
                ip,
                prefix_len,
                gateway,
                dns_servers,
            },
        }
    }

    async fn next_event_static(&mut self) -> NetInterfaceEventStatic {
        loop {
            // First, check the out buffer.
            if !self.reported_available_data {
                if !self.ethernet.device_mut().device_out_buffer.is_empty() {
                    self.reported_available_data = true;
                    return NetInterfaceEventStatic::EthernetCableOut;
                }
            }

            // Check whether any socket has changed state by comparing the latest known state
            // with what `smoltcp` tells us.
            // TODO: make changes in smoltcp to make this better?
            if self.check_sockets_required {
                for (socket_id, socket_state) in &mut self.sockets_state {
                    let mut smoltcp_socket =
                        self.sockets.get::<smoltcp::socket::TcpSocket>(socket_id.0);

                    // Check if this socket got connected.
                    if !socket_state.is_connected && smoltcp_socket.may_send() {
                        socket_state.is_connected = true;

                        let local_endpoint = {
                            let endpoint = smoltcp_socket.local_endpoint();
                            debug_assert_ne!(endpoint.port, 0);
                            let ip = match endpoint.addr {
                                smoltcp::wire::IpAddress::Ipv4(addr) => {
                                    IpAddr::from(Ipv4Addr::from(addr))
                                }
                                smoltcp::wire::IpAddress::Ipv6(addr) => {
                                    IpAddr::from(Ipv6Addr::from(addr))
                                }
                                _ => unreachable!(),
                            };
                            SocketAddr::from((ip, endpoint.port))
                        };

                        let remote_endpoint = {
                            let endpoint = smoltcp_socket.remote_endpoint();
                            debug_assert_ne!(endpoint.port, 0);
                            let ip = match endpoint.addr {
                                smoltcp::wire::IpAddress::Ipv4(addr) => {
                                    IpAddr::from(Ipv4Addr::from(addr))
                                }
                                smoltcp::wire::IpAddress::Ipv6(addr) => {
                                    IpAddr::from(Ipv6Addr::from(addr))
                                }
                                _ => unreachable!(),
                            };
                            SocketAddr::from((ip, endpoint.port))
                        };

                        return NetInterfaceEventStatic::TcpConnected(
                            *socket_id,
                            local_endpoint,
                            remote_endpoint,
                        );
                    }

                    // Check if this socket got closed.
                    if !socket_state.is_closed && !smoltcp_socket.is_open() {
                        socket_state.is_closed = true;
                        let socket_id = *socket_id;
                        self.sockets_state.remove(&socket_id);
                        return NetInterfaceEventStatic::TcpClosed(socket_id);
                    }

                    // Check if this socket has data for reading.
                    if !socket_state.read_ready && smoltcp_socket.can_recv() {
                        socket_state.read_ready = true;
                        return NetInterfaceEventStatic::TcpReadReady(*socket_id);
                    }

                    // Continue writing `write_remaining`.
                    while smoltcp_socket.can_send() && !socket_state.write_remaining.is_empty() {
                        let written = smoltcp_socket
                            .send_slice(&socket_state.write_remaining)
                            .unwrap();
                        assert_ne!(written, 0);
                        self.ethernet_poll_delay = None;
                        socket_state.write_remaining =
                            socket_state.write_remaining.split_off(written);
                    }

                    // Report when this socket is available for writing.
                    if smoltcp_socket.may_send()
                        && !socket_state.write_ready
                        && socket_state.write_remaining.is_empty()
                    {
                        socket_state.write_ready = true;
                        return NetInterfaceEventStatic::TcpWriteFinished(*socket_id);
                    }
                }

                // Only set `check_sockets_required` to false here, when we have iterated through
                // all the sockets and made sure that nothing could be reported anymore.
                self.check_sockets_required = false;
            }

            // Perform an active wait if any is going on.
            {
                if let Some(ethernet_poll_delay) = self.ethernet_poll_delay.as_mut() {
                    ethernet_poll_delay.await;
                }
                self.ethernet_poll_delay = None;
            }

            // We don't want to query `now` too often, so do it only once, here.
            let now = now().await;

            // Errors other than `Unrecognized` are meant to be logged and ignored.
            match self.ethernet.poll(&mut self.sockets, now) {
                Ok(true) => self.check_sockets_required = true,
                Ok(false) => {}
                // The documentation of smoltcp recommends to *not* log any `Unrecognized`
                // error, as such errors happen very frequently.
                Err(smoltcp::Error::Unrecognized) => {}
                Err(err) => {
                    log::trace!("Error while polling interface: {:?}", err);
                }
            };

            // Process the DHCPv4 client.
            // The documentation mentions that this must be done *after* polling the interface.
            if let Some(dhcp_v4_client) = &mut self.dhcp_v4_client {
                match dhcp_v4_client.poll(&mut self.ethernet, &mut self.sockets, now) {
                    Err(smoltcp::Error::Unrecognized) => {}
                    Err(err) => {
                        log::trace!("Error while polling DHCP client: {:?}", err);
                    }
                    Ok(None) => {}
                    Ok(Some(config)) => {
                        // Update the configuration of the Ethernet state machine with the DHCP
                        // discovery.
                        if let Some(address) = config.address.as_ref() {
                            self.ethernet.update_ip_addrs(|addrs| {
                                *addrs.iter_mut().nth(0).unwrap() = address.clone().into();
                            });
                        }
                        if let Some(router) = config.router.as_ref() {
                            self.ethernet
                                .routes_mut()
                                .add_default_ipv4_route(router.clone().into())
                                .unwrap();
                        }

                        // Report that to the user.
                        // TODO: is it possible to get multiple independent reports in such as
                        // way that we never emit the DhcpDiscovery event?
                        if let (Some(address), Some(router)) =
                            (config.address.as_ref(), config.router.as_ref())
                        {
                            // Now that the DHCP request is complete, drop the client.
                            // TODO: added as a hack, is that correct?
                            self.dhcp_v4_client = None;

                            return NetInterfaceEventStatic::DhcpDiscovery {
                                ip: address.address().into(),
                                prefix_len: address.prefix_len(),
                                gateway: router.clone().into(),
                                dns_servers: config
                                    .dns_servers
                                    .iter()
                                    .filter_map(|d| d.clone())
                                    .map(Ipv4Addr::from)
                                    .collect(),
                            };
                        }
                    }
                }
            }

            // Update `ethernet_poll_delay`.
            debug_assert!(self.ethernet_poll_delay.is_none());
            self.ethernet_poll_delay = Some({
                let when_iface = self.ethernet.poll_delay(&mut self.sockets, now);
                let when_dchp = self.dhcp_v4_client.as_ref().map(|c| c.next_poll(now));
                let combined = match (when_iface, when_dchp) {
                    (Some(a), Some(b)) => cmp::min(a, b),
                    (Some(a), None) => a,
                    (None, Some(b)) => b,
                    // `(None, None)` means "no deadline", other words "infinite". For convenience,
                    // we instead set an arbitrary deadline.
                    (None, None) => smoltcp::time::Duration::from_secs(20),
                };

                redshirt_time_interface::Delay::new(combined.into())
            });
        }
    }
}

impl<TSockUd> fmt::Debug for NetInterfaceState<TSockUd> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("NetInterfaceState").finish()
    }
}

impl<'a, TSockUd> TcpSocket<'a, TSockUd> {
    /// Returns the unique identifier of this socket.
    pub fn id(&self) -> SocketId {
        self.id
    }

    /// Starts the process of closing the TCP socket.
    pub fn close(&mut self) {
        let mut socket = self
            .interface
            .sockets
            .get::<smoltcp::socket::TcpSocket<'static>>(self.id.0);
        socket.close();
        self.interface.ethernet_poll_delay = None;
    }

    /// Instantly drops the socket without a proper shutdown.
    pub fn reset(self) {
        let mut socket = self
            .interface
            .sockets
            .get::<smoltcp::socket::TcpSocket<'static>>(self.id.0);
        socket.abort();
        self.interface
            .sockets_state
            .get_mut(&self.id)
            .unwrap()
            .is_closed = true;
    }

    /// Reads the data that has been received on the TCP socket.
    ///
    /// Returns an empty `Vec` if there is no data available.
    pub fn read(&mut self) -> Vec<u8> {
        let mut socket = self
            .interface
            .sockets
            .get::<smoltcp::socket::TcpSocket<'static>>(self.id.0);
        if !socket.can_recv() {
            return Vec::new();
        }

        self.interface
            .sockets_state
            .get_mut(&self.id)
            .unwrap()
            .read_ready = false;

        let recv_queue_len = socket.recv_queue();
        let mut out = Vec::with_capacity(recv_queue_len);
        unsafe {
            out.set_len(recv_queue_len);
        }
        let n_recved = socket.recv_slice(&mut out).unwrap();
        debug_assert_eq!(n_recved, recv_queue_len);
        debug_assert_eq!(socket.recv_queue(), 0);
        out
    }

    /// Passes a buffer that the socket will encode into Ethernet frames.
    ///
    /// Only one buffer can be active at any given point in time. If a buffer is already active,
    /// returns `Err(buffer)`.
    pub fn set_write_buffer(&mut self, mut buffer: Vec<u8>) -> Result<(), Vec<u8>> {
        let mut state = self.interface.sockets_state.get_mut(&self.id).unwrap();
        if !state.write_remaining.is_empty() {
            return Err(buffer);
        }

        let mut socket = self
            .interface
            .sockets
            .get::<smoltcp::socket::TcpSocket<'static>>(self.id.0);

        if socket.can_send() {
            let written = socket.send_slice(&buffer).unwrap();
            self.interface.ethernet_poll_delay = None;
            buffer = buffer.split_off(written);
        }

        state.write_ready = false;
        state.write_remaining = buffer;
        self.interface.check_sockets_required = true;
        Ok(())
    }

    /// Returns a reference to the user data stored within the socket state.
    pub fn user_data(&self) -> &TSockUd {
        let mut state = self.interface.sockets_state.get(&self.id).unwrap();
        &state.user_data
    }

    /// Returns a reference to the user data stored within the socket state.
    pub fn user_data_mut(&mut self) -> &mut TSockUd {
        let mut state = self.interface.sockets_state.get_mut(&self.id).unwrap();
        &mut state.user_data
    }

    /// Internal function that returns the `smoltcp::socket::TcpSocket` contained within the set.
    fn smoltcp_socket(
        &mut self,
    ) -> smoltcp::socket::SocketRef<smoltcp::socket::TcpSocket<'static>> {
        self.interface
            .sockets
            .get::<smoltcp::socket::TcpSocket<'static>>(self.id.0)
    }
}

impl<'a, TSockUd> fmt::Debug for TcpSocket<'a, TSockUd> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("TcpSocket").field(&self.id()).finish()
    }
}

// TODO: remove?
async fn now() -> smoltcp::time::Instant {
    let now = redshirt_time_interface::monotonic_clock().await;
    smoltcp::time::Instant::from_millis(i64::try_from(now / 1_000_000).unwrap())
}

/// Implementation of `smoltcp::phy::Device`.
struct RawDevice {
    /// Buffer of data to send out to the virtual Ethernet cable.
    device_out_buffer: Vec<u8>,

    /// Buffer of data received from the virtual Ethernet cable.
    device_in_buffer: Vec<u8>,
}

impl<'a> smoltcp::phy::Device<'a> for RawDevice {
    type RxToken = RawDeviceRxToken<'a>;
    type TxToken = RawDeviceTxToken<'a>;

    fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
        if self.device_in_buffer.is_empty() {
            return None;
        }

        if !self.device_out_buffer.is_empty() {
            return None;
        }

        let rx = RawDeviceRxToken {
            buffer: &mut self.device_in_buffer,
        };
        let tx = RawDeviceTxToken {
            buffer: &mut self.device_out_buffer,
        };
        Some((rx, tx))
    }

    fn transmit(&'a mut self) -> Option<Self::TxToken> {
        if !self.device_out_buffer.is_empty() {
            return None;
        }

        Some(RawDeviceTxToken {
            buffer: &mut self.device_out_buffer,
        })
    }

    fn capabilities(&self) -> phy::DeviceCapabilities {
        let mut caps: phy::DeviceCapabilities = Default::default();
        caps.max_transmission_unit = 9216; // FIXME:
        caps.max_burst_size = None;
        caps.checksum = phy::ChecksumCapabilities::ignored();
        caps.checksum.ipv4 = phy::Checksum::Both;
        caps.checksum.udp = phy::Checksum::Both;
        caps.checksum.tcp = phy::Checksum::Both;
        caps.checksum.icmpv4 = phy::Checksum::Both;
        caps.checksum.icmpv6 = phy::Checksum::Both;
        caps
    }
}

struct RawDeviceRxToken<'a> {
    buffer: &'a mut Vec<u8>,
}

impl<'a> phy::RxToken for RawDeviceRxToken<'a> {
    fn consume<R, F>(mut self, timestamp: Instant, f: F) -> Result<R, smoltcp::Error>
    where
        F: FnOnce(&mut [u8]) -> Result<R, smoltcp::Error>,
    {
        let result = f(&mut self.buffer);
        self.buffer.clear();
        result
    }
}

struct RawDeviceTxToken<'a> {
    buffer: &'a mut Vec<u8>,
}

impl<'a> phy::TxToken for RawDeviceTxToken<'a> {
    fn consume<R, F>(mut self, timestamp: Instant, len: usize, f: F) -> Result<R, smoltcp::Error>
    where
        F: FnOnce(&mut [u8]) -> Result<R, smoltcp::Error>,
    {
        debug_assert!(self.buffer.is_empty());
        *self.buffer = Vec::with_capacity(len);
        unsafe {
            self.buffer.set_len(len);
        }
        f(&mut self.buffer)
    }
}
