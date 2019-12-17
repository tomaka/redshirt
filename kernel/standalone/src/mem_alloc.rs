// Copyright (C) 2019  Pierre Krieger
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

use core::ops::Range;

/// Initialize the memory allocator.
///
/// After this function returns, the memory allocator will use the memory range passed as
/// parameter.
///
/// # Panics
///
/// Panics if `range.end` is inferior to `range.start`.
///
/// # Safety
///
/// The memory range has to be RAM or behave like RAM (i.e. both readable and writable,
/// consistent, and so on). This memory range must not be touched by anything (other than the
/// allocator) afterwards.
///
pub unsafe fn initialize(range: Range<usize>) {
    assert!(range.end >= range.start);
    ALLOCATOR.lock().init(range.start, range.end - range.start);
}

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

#[alloc_error_handler]
fn alloc_error_handler(_: core::alloc::Layout) -> ! {
    panic!()
}
