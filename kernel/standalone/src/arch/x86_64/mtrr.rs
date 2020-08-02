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

//! # Caching
//!
//! There exists six ways a CPU can treat memory, w.r.t. caching:
//!
//! - Strong Uncachable (UC). Memory is not cached. All operations are ordered.
//! - Uncachable (UC-). Used in conjunction with the (Page Attributes Table) PAT, which we don't
//! support.
//! - Write Combining (WC). Memory is not cached. Writes might be reordered and multiple writes
//! might be combined into one.
//! - Write Through (WT). Memory is cached. Writes update the cache entry (if any) and are also
//! propagated to the physical memory and may be combined (as with WC).
//! - Write Protected (WP). Similar to WT, but writes also invalidate the corresponding cache lines
//! on all other processors.
//! - Write Back (WB). Memory is cached. Writes only update the cache entry, and the physical
//! memory is updated only when the cache line is flushed (such as when the cache is full).
