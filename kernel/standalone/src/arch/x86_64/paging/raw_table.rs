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

use super::entry::EncodedEntry;
use core::{
    fmt,
    ops::{Index, IndexMut},
};

#[cfg(target_arch = "x86_64")]
const ENTRIES_PER_TABLE: usize = 512;
#[cfg(target_arch = "x86")]
const ENTRIES_PER_TABLE: usize = 1024;
const LAYOUT: alloc::alloc::Layout =
    unsafe { alloc::alloc::Layout::from_size_align_unchecked(4096, 4096) };

/// Represents a PML4, PDPT, PD or PT. In other words, a table used in the paging system.
///
/// Note that this API is entirely safe despite the fact that we modify the memory's layout,
/// which is an extremely unsafe thing to do. The safety is covered when loading a table into
/// the CR3 register.
pub struct RawPageTable {
    /// Because we have some alignment constraints, we need to allocate manually.
    table: *mut usize,
}

impl RawPageTable {
    /// Allocates a new table. The table is zeroed out.
    pub fn new() -> RawPageTable {
        unsafe {
            let ptr = alloc::alloc::alloc_zeroed(LAYOUT);
            assert!(!ptr.is_null());
            RawPageTable {
                table: ptr as *mut _,
            }
        }
    }

    /// Returns the address of the table in memory. Guaranteed to never change and to be a
    /// multiple of 4096.
    pub fn address(&self) -> usize {
        self.table as usize
    }
}

impl fmt::Debug for RawPageTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RawPageTable(0x{:x})", self.address())
    }
}

impl Drop for RawPageTable {
    fn drop(&mut self) {
        unsafe {
            alloc::alloc::dealloc(self.table as *mut u8, LAYOUT);
        }
    }
}

impl Index<usize> for RawPageTable {
    type Output = EncodedEntry;

    fn index(&self, idx: usize) -> &EncodedEntry {
        unsafe {
            assert!(idx < ENTRIES_PER_TABLE);
            &*(self.table.add(idx) as *mut EncodedEntry)
        }
    }
}

impl IndexMut<usize> for RawPageTable {
    fn index_mut(&mut self, idx: usize) -> &mut EncodedEntry {
        unsafe {
            assert!(idx < ENTRIES_PER_TABLE);
            &mut *(self.table.add(idx) as *mut EncodedEntry)
        }
    }
}
