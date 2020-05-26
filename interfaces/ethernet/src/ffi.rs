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

use parity_scale_codec::{Decode, Encode};
use redshirt_syscalls::InterfaceHash;

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0x56, 0xf0, 0xad, 0x54, 0x6c, 0x6d, 0x91, 0xce, 0xc2, 0x10, 0x88, 0xf6, 0x32, 0x2b, 0x66, 0x45,
    0xd4, 0xcf, 0xbe, 0xa3, 0xf7, 0x03, 0x13, 0xcd, 0x04, 0x65, 0xfd, 0x7f, 0x06, 0xd4, 0x24, 0xa1,
]);

#[derive(Debug, Encode, Decode)]
pub enum NetworkMessage {
    /// Notify of the existence of a new Ethernet interface.
    // TODO: what if this id was already registered?
    RegisterInterface {
        /// Unique per-process identifier.
        id: u64,
        /// MAC address of the interface.
        mac_address: [u8; 6],
    },

    /// Removes a previously-registered interface.
    UnregisterInterface(u64),

    /// Notify when an interface has received data (e.g. from the outside world). Must answer with
    /// a `()` when the send is finished and we're ready to accept a new packet.
    ///
    /// The packet must be an Ethernet frame without the CRC.
    InterfaceOnData(u64, Vec<u8>),

    /// Asks for the next packet of data to send out through this interface (e.g. going towards
    /// the outside world). Must answer with a `Vec<u8>`.
    ///
    /// The packet must be an Ethernet frame without the CRC.
    InterfaceWaitData(u64),
}
