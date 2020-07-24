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

// TODO: documentation

pub(crate) static GDT_TABLE: GdtTable =
    GdtTable([0, (1 << 53) | (1 << 47) | (1 << 44) | (1 << 43)]);

pub(crate) struct GdtTable([u64; 2]);

#[repr(align(8))]
pub(crate) struct GdtPtr(GdtPtrIn);

#[repr(packed)]
struct GdtPtrIn {
    size: u16,
    pointer: *const GdtTable,
}

unsafe impl Send for GdtPtr {}
unsafe impl Sync for GdtPtr {}

pub(crate) static GDT_POINTER: GdtPtr = GdtPtr(GdtPtrIn {
    size: 15,
    pointer: &GDT_TABLE,
});
