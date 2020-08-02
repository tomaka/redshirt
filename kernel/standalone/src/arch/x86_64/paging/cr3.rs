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

/// Loads the given page tables as the current PML4.
///
/// # Safety
///
/// The entries in the table must be valid and must remain valid if they get modified.
/// The table must not be destroyed. None of the other tables it references must be destroyed.
// TODO: explain flags
pub unsafe fn load_cr3(pml4: &RawPageTable, write_through: bool, cache_disable: bool) {
    debug_assert_eq!(pml4.address() % 4 * 1024, 0);

    let value = pml4.address()
        | (if write_through { 1 } else { 0 } << 3)
        | (if cache_disable { 1 } else { 0 } << 4);
    asm!("mov cr3, {}", in(reg) value, options(preserves_flags));
}
