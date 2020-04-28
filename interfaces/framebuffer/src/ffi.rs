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
//! in an input event whose format isn't properly defined yet. Sorry, it's kind of useless,
//! but well 🤷
// TODO: define input event

use redshirt_syscalls::InterfaceHash;

// TODO: split interface in two? one with inputs and one without?
// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0xfc, 0x60, 0x2e, 0x6e, 0xf2, 0x43, 0x9c, 0xa0, 0x40, 0x88, 0x81, 0x7d, 0xe5, 0xaf, 0xb6, 0x90,
    0x9e, 0x57, 0xc6, 0xc2, 0x5e, 0xbf, 0x02, 0x5b, 0x87, 0x7f, 0xaa, 0xae, 0xbe, 0xd5, 0x19, 0x9c,
]);
