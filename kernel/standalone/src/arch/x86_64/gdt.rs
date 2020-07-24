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

//! In 32bits and 64bits mode, x86/x86_64 processors must be configured with a GDT (Global
//! Descriptor Table).
//!
//! In 32bits protected mode, the GDT makes it possible to split the physical memory into
//! segments and to restrict non-priviledged code to certain segments. In practice, this
//! mechanism was barely used.
//! In 64bits long mode, the GDT must contain only one dummy descriptor.
//!
//! The code below contains the table whose pointer can be passed to all the CPUs. The table
//! contains one zero entry and one dummy entry, as required by the specifications.

/// Actual segment descriptors table.
pub(crate) static GDT_TABLE: GdtTable =
    GdtTable([0, (1 << 53) | (1 << 47) | (1 << 44) | (1 << 43)]);

/// Type of the GDT table.
pub(crate) struct GdtTable([u64; 2]);

/// Pointer to [`GDT_TABLE`] suitable for the `lgdt` instruction.
pub(crate) static GDT_POINTER: GdtPtr = GdtPtr(GdtPtrIn {
    size: 15,
    pointer: &GDT_TABLE,
});

/// Type of the GDT pointer.
#[repr(align(8))]
pub(crate) struct GdtPtr(GdtPtrIn);

// We need a second inner type in order to be able to apply both `repr(packed)` and
// `repr(align(8))`.
#[repr(packed)]
struct GdtPtrIn {
    size: u16,
    // TODO: **must** be 64bits, I believe?
    pointer: *const GdtTable,
}

unsafe impl Send for GdtPtr {}
unsafe impl Sync for GdtPtr {}
