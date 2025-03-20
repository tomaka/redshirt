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

use parity_scale_codec::{Decode, Encode};
use redshirt_syscalls::InterfaceHash;

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0x99, 0x94, 0xca, 0x60, 0xcf, 0x73, 0x7b, 0x59, 0xf7, 0xdc, 0x0c, 0xc4, 0xf0, 0x57, 0x42, 0x2e,
    0x79, 0xa7, 0xb6, 0x81, 0xbb, 0xf8, 0x4e, 0x24, 0x8e, 0xbf, 0x1a, 0x8f, 0x2c, 0xf6, 0xea, 0xc8,
]);

#[derive(Debug, Encode, Decode)]
pub enum DiskMessage {
    /// Notify of the existence of a new disk.
    // TODO: what if this id was already registered?
    RegisterDisk {
        /// Unique per-process identifier.
        id: u64,
        /// True if writing to this disk is allowed.
        allow_write: bool,
        /// Size, in bytes, of a sector of the disk.
        sector_size: u32,
        /// Number of sectors on the disk.
        num_sectors: u32,
    },

    /// Removes a previously-registered disk.
    UnregisterDisk(u64),

    /// Asks for the next command the disk must execute.
    ///
    /// Must answer with a [`DiskCommand`].
    DiskNextCommand(u64),

    /// Report that a [`DiskCommand::StartRead`] has finished.
    ///
    /// Has no response.
    ReadFinished(ReadId, Vec<u8>),

    /// Report that a [`DiskCommand::StartWrite`] has finished.
    ///
    /// Has no response.
    WriteFinished(WriteId),
}

#[derive(Debug, Encode, Decode)]
pub enum DiskCommand {
    StartRead {
        id: ReadId,
        sector_lba: u64,
        num_sectors: u32,
    },
    StartWrite {
        id: WriteId,
        sector_lba: u64,
        data: Vec<u8>,
    },
}

#[derive(Debug, Encode, Decode, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReadId(pub u64);

#[derive(Debug, Encode, Decode, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WriteId(pub u64);
