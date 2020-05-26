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

//!
//!
//! Message format:
//!
//! The type of message depends on the first byte:
//!
//! - 0: Creates a framebuffer. Next 4 bytes are a "framebuffer ID" as decided by the message
//! emitter. Next 4 bytes are the width in little endian. Next 4 bytes are the height in little
//! endian.
//! - 1: Destroys a framebuffer. Next 4 bytes are the framebuffer ID.
//! - 2: Set framebuffer content. Next 4 bytes are the framebuffer ID. The rest is 3 * width *
//! height values. The rest is RGB triplets.
//! - 3: Send back the next input event. Next 4 bytes are the framebuffer ID. The answer consists
//! in an input event whose format is a SCALE-encoding of the [`Event`] struct below.
//!
//! There actually exists two interfaces that use the same messages format: with events, or without
//! events. Messages whose first byte is `3` are invalid in the "without events" interface.

use redshirt_syscalls::InterfaceHash;

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE_WITH_EVENTS: InterfaceHash = InterfaceHash::from_raw_hash([
    0xfc, 0x60, 0x2e, 0x6e, 0xf2, 0x43, 0x9c, 0xa0, 0x40, 0x88, 0x81, 0x7d, 0xe5, 0xaf, 0xb6, 0x90,
    0x9e, 0x57, 0xc6, 0xc2, 0x5e, 0xbf, 0x02, 0x5b, 0x87, 0x7f, 0xaa, 0xae, 0xbe, 0xd5, 0x19, 0x9c,
]);

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE_WITHOUT_EVENTS: InterfaceHash = InterfaceHash::from_raw_hash([
    0xdf, 0x67, 0x74, 0x34, 0xd8, 0x0d, 0xc5, 0x9e, 0xf0, 0x6e, 0xb9, 0x44, 0xce, 0xaa, 0xc4, 0xde,
    0x8d, 0x2f, 0xdf, 0x39, 0x0a, 0xe6, 0xa8, 0x29, 0x3c, 0x8f, 0x88, 0x76, 0x5b, 0xe9, 0x1c, 0x70,
]);

/// Event that can be reported by a framebuffer.
///
/// > **Note**: These events are designed to take into account the possibility that some events are
/// >           lost. This can happen if the recipient queues messages too slowly.
#[derive(Debug, Clone, parity_scale_codec::Encode, parity_scale_codec::Decode)]
pub enum Event {
    KeyboardChange {
        /// Scancode as defined in the USB HID Usage tables.
        ///
        /// See table 12 on page 53:
        /// https://www.usb.org/sites/default/files/documents/hut1_12v2.pdf
        scancode: u16,

        /// New state of the given key.
        new_state: Keystate,
    },
}

#[derive(Debug, Clone, parity_scale_codec::Encode, parity_scale_codec::Decode)]
pub enum Keystate {
    Pressed,
    Released,
}
