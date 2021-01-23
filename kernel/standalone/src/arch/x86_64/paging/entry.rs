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

#[cfg(target_arch = "x86_64")]
pub const ENTRIES_PER_TABLE: usize = 512;
#[cfg(target_arch = "x86")]
pub const ENTRIES_PER_TABLE: usize = 1024;

// TODO: move somewhere
#[repr(align(4096))]
pub struct Table(pub [EncodedEntry; ENTRIES_PER_TABLE]);

impl Table {
    pub const fn empty() -> Self {
        Table([Absent.encode(); ENTRIES_PER_TABLE])
    }
}

/// Represents a PML4E, PDPTE, PDE, or PTE. In other words, an entry in a table used in the
/// paging system.
#[derive(Debug, Copy, Clone)] // TODO: better Debug impl
#[repr(transparent)]
pub struct EncodedEntry(usize);

impl EncodedEntry {
    pub const fn raw_value(&self) -> usize {
        self.0
    }
}

#[derive(Debug, derive_more::Display)]
pub enum DecodeError {
    InvalidTy,
}

/// Represents an absent entry.
///
/// Can be turned into an [`EncodedEntry`] with the [`Absent::encode`] method.
pub struct Absent;

impl Absent {
    /// Encodes this [`Absent`] into an entry ready for usage.
    pub const fn encode(self) -> EncodedEntry {
        EncodedEntry(0)
    }
}

impl From<Absent> for EncodedEntry {
    fn from(absent: Absent) -> EncodedEntry {
        absent.encode()
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

impl DecodedPml4ePdptePde {
    /// Encodes this descriptor into an entry ready for usage.
    ///
    /// # Safety
    ///
    /// For the entry to be valid:
    ///
    /// - `physical_address` must be a multiple of 4096.
    ///
    pub const unsafe fn encode_unchecked(self) -> EncodedEntry {
        self.into_decode_all().encode_unchecked()
    }

    /// Turns this structure into a less specific [`DecodeAll`].
    const fn into_decode_all(self) -> DecodedAll {
        DecodedAll {
            present: self.present,
            read_write: self.read_write,
            user: self.user,
            write_through: self.write_through,
            cache_disable: self.cache_disable,
            accessed: self.accessed,
            dirty: false,
            bit7: false,
            global: false,
            bit12: false,
            physical_address: self.physical_address,
            protection_key: 0,
            execute_disable: self.execute_disable,
        }
    }
}

impl TryFrom<DecodedPml4ePdptePde> for EncodedEntry {
    type Error = ();
    fn try_from(decoded: DecodedPml4ePdptePde) -> Result<Self, Self::Error> {
        TryFrom::try_from(decoded.into_decode_all())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

impl DecodedPdpte1G {
    /// Encodes this descriptor into an entry ready for usage.
    ///
    /// # Safety
    ///
    /// For the entry to be valid:
    ///
    /// - `physical_address` must be aligned on 1 GiB.
    ///
    pub const unsafe fn encode_unchecked(self) -> EncodedEntry {
        self.into_decode_all().encode_unchecked()
    }

    /// Turns this structure into a less specific [`DecodeAll`].
    const fn into_decode_all(self) -> DecodedAll {
        DecodedAll {
            present: self.present,
            read_write: self.read_write,
            user: self.user,
            write_through: self.write_through,
            cache_disable: self.cache_disable,
            accessed: self.accessed,
            dirty: self.dirty,
            bit7: true,
            global: self.global,
            bit12: self.attributes_table,
            physical_address: self.physical_address,
            protection_key: self.protection_key,
            execute_disable: self.execute_disable,
        }
    }
}

impl TryFrom<DecodedPdpte1G> for EncodedEntry {
    type Error = ();
    fn try_from(decoded: DecodedPdpte1G) -> Result<Self, Self::Error> {
        if decoded.physical_address % (1024 * 1024 * 1024) != 0 {
            return Err(());
        }

        TryFrom::try_from(decoded.into_decode_all())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

impl DecodedPde2M {
    /// Encodes this descriptor into an entry ready for usage.
    ///
    /// # Safety
    ///
    /// For the entry to be valid:
    ///
    /// - `physical_address` must be aligned on 2 Mib.
    ///
    pub const unsafe fn encode_unchecked(self) -> EncodedEntry {
        self.into_decode_all().encode_unchecked()
    }

    /// Turns this structure into a less specific [`DecodeAll`].
    const fn into_decode_all(self) -> DecodedAll {
        DecodedAll {
            present: self.present,
            read_write: self.read_write,
            user: self.user,
            write_through: self.write_through,
            cache_disable: self.cache_disable,
            accessed: self.accessed,
            dirty: self.dirty,
            bit7: true,
            global: self.global,
            bit12: self.attributes_table,
            physical_address: self.physical_address,
            protection_key: self.protection_key,
            execute_disable: self.execute_disable,
        }
    }
}

impl TryFrom<DecodedPde2M> for EncodedEntry {
    type Error = ();
    fn try_from(decoded: DecodedPde2M) -> Result<Self, Self::Error> {
        if decoded.physical_address % (2 * 1024 * 1024) != 0 {
            return Err(());
        }

        TryFrom::try_from(decoded.into_decode_all())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

impl DecodedPde4M {
    /// Encodes this descriptor into an entry ready for usage.
    ///
    /// # Safety
    ///
    /// For the entry to be valid:
    ///
    /// - `physical_address` must be aligned on 4 MiB.
    ///
    pub const unsafe fn encode_unchecked(self) -> EncodedEntry {
        self.into_decode_all().encode_unchecked()
    }

    /// Turns this structure into a less specific [`DecodeAll`].
    const fn into_decode_all(self) -> DecodedAll {
        DecodedAll {
            present: self.present,
            read_write: self.read_write,
            user: self.user,
            write_through: self.write_through,
            cache_disable: self.cache_disable,
            accessed: self.accessed,
            dirty: self.dirty,
            bit7: true,
            global: self.global,
            bit12: self.attributes_table,
            physical_address: self.physical_address,
            protection_key: 0,
            execute_disable: false,
        }
    }
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

        TryFrom::try_from(decoded.into_decode_all())
    }
}

/// Intermediary type common to all types of entries.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

        Ok(unsafe { decoded.encode_unchecked() })
    }
}

impl DecodedAll {
    const unsafe fn encode_unchecked(self) -> EncodedEntry {
        let value = (if self.present { 1 } else { 0 } << 0)
            | (if self.read_write { 1 } else { 0 } << 1)
            | (if self.user { 1 } else { 0 } << 2)
            | (if self.write_through { 1 } else { 0 } << 3)
            | (if self.cache_disable { 1 } else { 0 } << 4)
            | (if self.accessed { 1 } else { 0 } << 5)
            | (if self.dirty { 1 } else { 0 } << 6)
            | (if self.bit7 { 1 } else { 0 } << 7)
            | (if self.global { 1 } else { 0 } << 8)
            | (if self.bit12 { 1 } else { 0 } << 12)
            | self.physical_address
            | ((self.protection_key as usize) << 59)
            | (if self.execute_disable { 1 } else { 0 } << 63);

        EncodedEntry(value)
    }
}
