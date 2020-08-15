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

//! In the 16bits and 32bits modes of the x86/x86_64 architecture, memory can be divided into
//! *segments*. Each segment has a base address, a length, and a few miscellaneous
//! characteristics.
//!
//! In 32bits modes, the processor holds in a register called the GDTR the location of a data
//! structure named the GDT (Global Descriptor Table). When the CS, DS, ES, FS, GS, or SS register
//! is loaded with a non-zero value, the processor loads from the GDT the characteristics of the
//! corresponding segment.
//!
//! Segmentation is a deprecated feature in 64bits mode. While loading the value of the segment
//! registers works the same ways as in 32bits mode, the processor then ignores these registers
//! altogether when performing memory accesses.
//! For this reason, most operating systems don't rely on memory segmentation, even in 32bits
//! modes.
//!
//! However, because processors don't immediately start in 64bits mode, one can never completely
//! ignore segmentation.
//! In order to switch to 64bits mode, one must load the CS register with a segment whose
//! descriptor has a certain bit set. This bit determined whether the processor should run in
//! extended-32bits or 64bits mode.

/// Global Descriptor Table (GDT) with two entries:
///
/// - Since loading a zero value in a segment register doesn't perform any actual loading, the
/// first entry is a dummy entry.
/// - The second entry is a code segment with the `L` bit set to 1, indicating a 64-bit code
/// segment.
///
/// In order to switch to 64bits mode, load the `GDTR` with this table, then load the value `8` in
/// the `CS` register.
///
/// The memory address is guaranteed to fit in 32bits.
pub static GDT: Gdt = Gdt([0, (1 << 53) | (1 << 47) | (1 << 44) | (1 << 43)]);

// TODO: assert that GDT has a 32bits memory address, once Rust makes this possible
//       see https://github.com/rust-lang/rust/issues/51910

/// Opaque type of the GDT table.
pub struct Gdt([u64; 2]);

/// Pointer to [`GDT`] suitable for the `lgdt` instruction.
///
/// # 32-bit vs 64-bit LGDT instruction
///
/// In 32-bit mode the LGDT instruction expects a 32-bits GDT address, while in 64-bit mode it
/// expects as 64-bits GDT address.
///
/// The pointer below contains a 64-bits-long address, but with an address that is guaranteed to
/// fit in 32 bits. In other words, the 32 upper bits are 0s. Since x86/x86_64 is a little endian
/// platform, this pointer works for both the 32-bits and 64-bits versions of the LGDT
/// instruction.
///
/// # Example
///
/// ```ignore
/// asm!("lgdt {gdt_ptr}", gdt_ptr = sym GDT_POINTER);
/// ```
///
pub static GDT_POINTER: GdtPtr = GdtPtr(GdtPtr2 {
    _size: 15,
    _pointer: &GDT,
});

/// Opaque type of the GDT pointer.
#[repr(align(8))]
pub struct GdtPtr(GdtPtr2);

// We need a second inner type in order to be able to apply both `repr(packed)` and
// `repr(align(8))`.
#[repr(packed)]
struct GdtPtr2 {
    _size: u16,
    // TODO: must be 64bits, as explained above; see https://github.com/rust-lang/rust/issues/51910
    _pointer: *const Gdt,
}

// TODO: remove once `GdtPtr2::_pointer` is a u64
unsafe impl Send for GdtPtr {}
unsafe impl Sync for GdtPtr {}
