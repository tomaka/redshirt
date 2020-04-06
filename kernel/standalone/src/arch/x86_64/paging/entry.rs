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

use core::convert::TryFrom;

/// Represents a PML4E, PDPTE, PDE, or PTE. In other words, an entry in a table used in the
/// paging system.
#[derive(Debug, Copy, Clone)] // TODO: better Debug impl
#[repr(transparent)]
pub struct EncodedEntry(usize);

impl EncodedEntry {
    pub fn raw_value(&self) -> usize {
        self.0
    }
}

// TODO: impl Display
#[derive(Debug)]
pub enum DecodeError {
    InvalidTy,
}

/// Represents an absent entry.
///
/// Can be turned into an [`EncodedEntry`] with the `From` trait.
pub struct Absent;

impl From<Absent> for EncodedEntry {
    fn from(_: Absent) -> EncodedEntry {
        EncodedEntry(0)
    }
}

impl TryFrom<EncodedEntry> for Absent {
    type Error = DecodeError;
    fn try_from(value: EncodedEntry) -> Result<Self, Self::Error> {
        if value.0 & 0x1 == 0 {
            Ok(Absent)
        } else {
            Err(DecodeError::InvalidTy)
        }
    }
}

pub struct DecodedPml4ePdptePde {
    pub present: bool,
    pub read_write: bool,
    pub user: bool,
    pub write_through: bool,
    pub cache_disable: bool,
    pub accessed: bool,
    pub physical_address: usize,
    pub execute_disable: bool,
}

impl TryFrom<DecodedPml4ePdptePde> for EncodedEntry {
    type Error = ();
    fn try_from(decoded: DecodedPml4ePdptePde) -> Result<Self, Self::Error> {
        TryFrom::try_from(DecodedAll {
            present: decoded.present,
            read_write: decoded.read_write,
            user: decoded.user,
            write_through: decoded.write_through,
            cache_disable: decoded.cache_disable,
            accessed: decoded.accessed,
            dirty: false,
            bit7: false,
            global: false,
            bit12: false,
            physical_address: decoded.physical_address,
            protection_key: 0,
            execute_disable: decoded.execute_disable,
        })
    }
}

pub struct DecodedPdpte1G {
    pub present: bool,
    pub read_write: bool,
    pub user: bool,
    pub write_through: bool,
    pub cache_disable: bool,
    pub accessed: bool,
    pub dirty: bool,
    pub global: bool,
    pub attributes_table: bool,
    pub physical_address: usize,
    pub protection_key: u8,
    pub execute_disable: bool,
}

impl TryFrom<DecodedPdpte1G> for EncodedEntry {
    type Error = ();
    fn try_from(decoded: DecodedPdpte1G) -> Result<Self, Self::Error> {
        if decoded.physical_address % (1024 * 1024 * 1024) != 0 {
            return Err(());
        }

        TryFrom::try_from(DecodedAll {
            present: decoded.present,
            read_write: decoded.read_write,
            user: decoded.user,
            write_through: decoded.write_through,
            cache_disable: decoded.cache_disable,
            accessed: decoded.accessed,
            dirty: decoded.dirty,
            bit7: true,
            global: decoded.global,
            bit12: decoded.attributes_table,
            physical_address: decoded.physical_address,
            protection_key: decoded.protection_key,
            execute_disable: decoded.execute_disable,
        })
    }
}

pub struct DecodedPde2M {
    pub present: bool,
    pub read_write: bool,
    pub user: bool,
    pub write_through: bool,
    pub cache_disable: bool,
    pub accessed: bool,
    pub dirty: bool,
    pub global: bool,
    pub attributes_table: bool,
    pub physical_address: usize,
    pub protection_key: u8,
    pub execute_disable: bool,
}

impl TryFrom<DecodedPde2M> for EncodedEntry {
    type Error = ();
    fn try_from(decoded: DecodedPde2M) -> Result<Self, Self::Error> {
        if decoded.physical_address % (2 * 1024 * 1024) != 0 {
            return Err(());
        }

        TryFrom::try_from(DecodedAll {
            present: decoded.present,
            read_write: decoded.read_write,
            user: decoded.user,
            write_through: decoded.write_through,
            cache_disable: decoded.cache_disable,
            accessed: decoded.accessed,
            dirty: decoded.dirty,
            bit7: true,
            global: decoded.global,
            bit12: decoded.attributes_table,
            physical_address: decoded.physical_address,
            protection_key: decoded.protection_key,
            execute_disable: decoded.execute_disable,
        })
    }
}

pub struct DecodedPde4M {
    pub present: bool,
    pub read_write: bool,
    pub user: bool,
    pub write_through: bool,
    pub cache_disable: bool,
    pub accessed: bool,
    pub dirty: bool,
    pub global: bool,
    pub attributes_table: bool,
    pub physical_address: usize,
}

impl TryFrom<DecodedPde4M> for EncodedEntry {
    type Error = ();
    fn try_from(decoded: DecodedPde4M) -> Result<Self, Self::Error> {
        if decoded.physical_address % (4 * 1024 * 1024) != 0 {
            return Err(());
        }

        // Note: we would normally add support for PAE here. PAE allow accessing 36bits of
        // physical memory as opposed to 32bits. Supporting PAE in an identity-mapped scheme,
        // however, makes little sense, as we could not actually access the extra available
        // physical memory.

        TryFrom::try_from(DecodedAll {
            present: decoded.present,
            read_write: decoded.read_write,
            user: decoded.user,
            write_through: decoded.write_through,
            cache_disable: decoded.cache_disable,
            accessed: decoded.accessed,
            dirty: decoded.dirty,
            bit7: true,
            global: decoded.global,
            bit12: decoded.attributes_table,
            physical_address: decoded.physical_address,
            protection_key: 0,
            execute_disable: false,
        })
    }
}

/// Intermediary type common to all types of entries.
struct DecodedAll {
    present: bool,
    read_write: bool,
    user: bool,
    write_through: bool,
    cache_disable: bool,
    accessed: bool,
    dirty: bool,
    bit7: bool,
    global: bool,
    bit12: bool,
    physical_address: usize,
    protection_key: u8,
    execute_disable: bool,
}

// TODO: implement From<EncodedEntry> for DecodedAll

impl TryFrom<DecodedAll> for EncodedEntry {
    type Error = ();
    fn try_from(decoded: DecodedAll) -> Result<Self, Self::Error> {
        // TODO: support 32bits as well
        // TODO: check physical_address against MAXPHYSADDR
        if decoded.physical_address % 0x1000 != 0 {
            return Err(());
        }
        if decoded.bit12 && (decoded.physical_address % 0x2000) != 0 {
            return Err(());
        }
        if decoded.protection_key >= 0x10 {
            return Err(());
        }

        let value = (if decoded.present { 1 } else { 0 } << 0)
            | (if decoded.read_write { 1 } else { 0 } << 1)
            | (if decoded.user { 1 } else { 0 } << 2)
            | (if decoded.write_through { 1 } else { 0 } << 3)
            | (if decoded.cache_disable { 1 } else { 0 } << 4)
            | (if decoded.accessed { 1 } else { 0 } << 5)
            | (if decoded.dirty { 1 } else { 0 } << 6)
            | (if decoded.bit7 { 1 } else { 0 } << 7)
            | (if decoded.global { 1 } else { 0 } << 8)
            | (if decoded.bit12 { 1 } else { 0 } << 12)
            | decoded.physical_address
            | (usize::from(decoded.protection_key) << 59)
            | (if decoded.execute_disable { 1 } else { 0 } << 63);

        Ok(EncodedEntry(value))
    }
}
