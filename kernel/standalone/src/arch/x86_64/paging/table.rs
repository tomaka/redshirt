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

use super::raw_table::RawPageTable;
use core::convert::TryFrom;
use hashbrown::HashMap;
use nohash_hasher::BuildNoHashHasher;

// TODO: support PAE caching in 32bits mode

// TODO: invlpg must be propagated to other CPUS /!\

// Saved for later
/*unsafe {
    asm!("invlpg ($0)" :: "r"(addr) : "memory");
}*/

/// Represents a PML4, PDPT, PD or PT. In other words, a table used in the paging system.
pub struct PageTable {
    raw_table: RawPageTable,

    /// Number of entries in the page table that are marked as "present".
    ///
    /// In other words, the entries in the table marked as present are the range
    /// `0..num_present_entries`.
    num_present_entries: u16,

    /// What kind of table we are.
    kind: VerifiedKind,

    /// Child entries pointed to by [`PageTable::raw_table`]. The keys are the index within the
    /// table.
    ///
    /// There is an entry in this hashmap if and only if the corresponding entry in the page
    /// table is pointing to the child table.
    children: HashMap<u16, PageTable, BuildNoHashHasher<u16>>,
}

pub enum Kind {
    PML4,
    PDPT { base: usize },
    PD { base: usize },
    PT { base: usize },
}

/// Equivalent to [`Kind`] but whose validity has been ensured.
pub struct VerifiedKind(Kind);

impl PageTable {
    pub fn empty(kind: VerifiedKind) -> PageTable {
        PageTable {
            raw_table: RawPageTable::new(),
            num_present_entries: 0,
            kind,
            children: HashMap::with_capacity_and_hasher(16, Default::default()),
        }
    }

    /// Ensures that entries `0` to `num` are marked as present.
    fn populate_to(&mut self, num: u16) {
        for n in self.num_present_entries..num {
            // TODO:
            //self.raw_table[n]
        }

        self.num_present_entries = num;
    }

    ///
    ///
    /// # Panic
    ///
    /// Panics if the entry is not already marked as "present" in the table.
    fn split(&mut self, entry: u16) {}
}
