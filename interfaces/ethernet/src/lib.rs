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

//! Ethernet interfaces.
//!
//! This interface allows registering Ethernet interfaces that conform to the requirements
//! described in [RFC1122](https://tools.ietf.org/html/rfc1122).
//!
//! Users of this interface can notify of the presence of an Ethernet interface, after which it
//! can notify of Ethernet frames having been received on this interface, and request Ethernet
//! frames to be sent on the interface.
//!
//! The Ethernet frames that are communicated contain:
//!
//! - The destination MAC.
//! - The source MAC.
//! - An optional 802.1Q tag.
//! - The Ethertype field.
//! - The payload.
//!
//! Most notably, they **don't** include any preamble, delimiter, interpacket gap, and checksum.
//!
//! # Networking overview
//!
//! This section provides some overview of networking in general.
//!
//! ## Link layer routing
//!
//! Each Ethernet interface registered with this interface possesses a MAC address, and is assumed
//! to be directly connected to zero, one, or more other machines.
//!
//! Thanks to the ARP or NDP protocols, it is possible to broadcast a request on the Ethernet
//! interface asking who we are connected to. The nodes that receive the request respond with their
//! IP and MAC address. If later we want to send a direct message to one of these nodes, we simply
//! emit an Ethernet frame with the MAC address of the destination.
//!
//! Note that being "directly connected" to a node doesn't necessarily mean that there exists a
//! physical cable between us and this node. Hubs, switches, or Wifi routers, can act as an
//! intermediary.
//!
//! ## IP layer routing
//!
//! All of the registered Ethernet interfaces are considered as being part of one single network
//! (typically the Internet).
//!
//! > **Note**: This doesn't mean that we are free to send out packets on any interface that we
//! >           want and expect the routing to make the packet find its way. In particular, this
//! >           single network can be disjoint.
//!
//! Nodes on this network (i.e. something with an IP address) can be split into two categories:
//!
//! - Nodes we are directly connected to, as described in the previous section. It is possible to
//! send a direct message to these nodes by writing bytes on the right interface.
//! - Nodes we are *not* directly connected to.
//!
//! The implementer of this interface maintains what we call a *routing table*. A routing table is
//! a collection of entries, each entry consisting of:
//!
//! - A "pattern", kind of similar to a regex for example. This pattern consists of an IP address
//! and mask. An IP address matches that pattern if
//! `(ip_addr & pattern.mask) == (pattern.ip & pattern.mask)`.
//! - A gateway. This is the IP address of a node that we are physically connected to and that is
//! in charge of relaying packets to nodes that match the given pattern.
//! - The network interface the gateway is connected to.
//!
//! When we want to reach the node with a specific IP address, we look through the table until
//! we find an entry whose pattern matches the target IP address. If multiple entries are matching,
//! we pick the one with the most specific mask.
//!
//! The gateway is then expected to properly direct the packet towards its rightful destination.
//!

pub mod ffi;
pub mod interface;
