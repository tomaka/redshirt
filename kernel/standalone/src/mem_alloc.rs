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
/// Pass to this function a list of memory ranges that are available for use.
///
/// After this function returns, you can use heap allocations.
///
/// # Panics
///
/// Panics if `range.end` is inferior to `range.start` for any of the elements.
///
/// # Safety
///
/// The memory ranges have to be RAM or behave like RAM (i.e. both readable and writable,
/// consistent, and so on). The memory ranges must not be touched by anything (other than the
/// allocator) afterwards.
///
pub unsafe fn initialize(ranges: impl Iterator<Item = Range<usize>>) {
    // We choose the largest range.
    let range = ranges.max_by_key(|r| {
        assert!(r.end >= r.start);
        r.end - r.start
    });

    let range = match range {
        Some(r) => r,
        // If the iterator was empty, return with initializing the allocator.
        None => return,
    };

    // Don't initialize the allocator if all the ranges were 0.
    if range.start == range.end {
        return;
    }

    assert!(range.end >= range.start);
    ALLOCATOR.lock().init(range.start, range.end - range.start);
}

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("allocation of 0x{:x} bytes failed", layout.size())
}
