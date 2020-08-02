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

use super::entry;

use alloc::boxed::Box;
use core::{
    fmt,
    ops::{Index, IndexMut},
};

/// Represents a PML4, PDPT, PD or PT. In other words, a table used in the paging system.
///
/// Note that this API is entirely marked as safe despite the fact that we modify the memory's
/// layout, which is an extremely unsafe thing to do. The safety is covered when loading a table
/// into the CR3 register.
pub struct RawPageTable {
    table: Box<entry::Table>,
}

impl RawPageTable {
    /// Allocates a new table. The table is zeroed out.
    pub fn new() -> RawPageTable {
        RawPageTable {
            table: Box::new(entry::Table::empty()),
        }
    }

    /// Returns the address of the table in memory. Guaranteed to never change and to be a
    /// multiple of 4096.
    pub fn address(&self) -> usize {
        self.table.0.as_ptr() as usize
    }
}

impl fmt::Debug for RawPageTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RawPageTable(0x{:x})", self.address())
    }
}

impl Index<usize> for RawPageTable {
    type Output = entry::EncodedEntry;

    fn index(&self, idx: usize) -> &entry::EncodedEntry {
        &(self.table.0)[idx]
    }
}

impl IndexMut<usize> for RawPageTable {
    fn index_mut(&mut self, idx: usize) -> &mut entry::EncodedEntry {
        &mut (self.table.0)[idx]
    }
}
