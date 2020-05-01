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

//! Networking.
//!
//! This documentation provides some overview of networking in general.
//!
//! # IP routing
//!
//! The handler of this interface maintains a list of active network interfaces. All of these
//! interfaces are considered as being part of one single network (typically, the Internet
//! network).
//!
//! > **Note**: This doesn't mean that we are free to send out packets on any interface that we
//! >           want and expect the routing to make the packet find its way. In particular, this
//! >           single network can be disjoint.
//!
//! Nodes (i.e. something with an IP address) on the network can be split into two categories:
//!
//! - Nodes we are physically connected to. It is possible to send a direct message to a these
//! nodes by writing bytes on the right interface.
//! - The others. Nodes we are *not* physically connected to.
//!
//! The handler of this interface contains what we call a *routing table*. A routing table is a
//! list of entries. Each entry consists of:
//!
//! - A "pattern", kind of similar to a regex for example. This pattern consists of an IP address
//! and mask. An IP address matches that pattern if
//! `(ip_addr & pattern.mask) == (pattern.ip & pattern.mask)`.
//! - A gateway. This is the IP address of a node that we are physically connected to and that is
//! in charge of relaying packets to nodes that match the given pattern.
//! - The interface the gateway is connected to.
//!
//! When we want to reach the node with a specific IP address, we look through the table until
//! we find an entry whose pattern matches the target IP address. If multiple entries are matching,
//! we pick the one with the most specific mask.
//!

#![deny(intra_doc_link_resolution_failure)]

pub mod ffi;
pub mod interface;
