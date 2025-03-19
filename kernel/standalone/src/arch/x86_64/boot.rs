// Copyright (C) 2019-2021  Pierre Krieger
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

//! This file contains the entry point of our kernel.
//!
//! Once the bootloader finishes loading the kernel (as an ELF file), it will run its entry point,
//! which is the `_start` function defined in this file.
//!
//! Since we are conforming to the multiboot2 specifications, the bootloader is expected to set the
//! ebx register to the memory address of a data structure containing information about the
//! environment.
//!
//! The environment in which we start in is the protected mode where the kernel is identity-mapped.
//!
//! The role of the `_start` function below is to perform some checks, set up everything that is
//! needed to run freestanding 64bits Rust code (i.e. a stack, paging, long mode), and call the
//! [`super::entry_point_step2`] Rust function.

#[macro_export]
macro_rules! __gen_boot {
    (
        entry: $entry:path,
        memory_zeroing_start: $memory_zeroing_start:path,
        memory_zeroing_end: $memory_zeroing_end:path,
    ) => {
        const _: () = {
            #[export_name = "efi_main"]
            pub extern "efiapi" fn entry_point_step1(
                handle: *mut core::ffi::c_void,
                system_table: *const core::ffi::c_void,
            ) -> usize {
                unsafe {
                    $crate::arch::x86_64::entry_point_step2(handle, system_table, $entry);
                    // This should never be reached.
                    core::arch::asm!("cli; hlt");
                    0
                }
            }
        };
    };
}

/// Used as a temporary variable during the boot process.
#[doc(hidden)]
pub static mut MULTIBOOT_INFO_PTR: u64 = 0;

#[doc(hidden)]
pub const MAIN_PROCESSOR_STACK_SIZE: usize = 0x800000;

/// Stack used by the main processor.
///
/// As per x64 calling convention, the stack pointer must always be a multiple of 16. The stack
/// must therefore have an alignment of 16 as well.
#[doc(hidden)]
#[repr(align(16), C)]
pub struct Stack([u8; MAIN_PROCESSOR_STACK_SIZE]);
pub static mut MAIN_PROCESSOR_STACK: Stack = Stack([0; MAIN_PROCESSOR_STACK_SIZE]);

// TODO: handle this in a more proper way
// TODO: fill the paging from the Rust code, and not in assembly

#[repr(align(0x1000), C)]
#[doc(hidden)]
#[derive(Copy, Clone)]
pub struct PagingEntry([u8; 0x1000]);
/// PML4. The entry point for our paging system.
#[doc(hidden)]
pub static mut PML4: PagingEntry = PagingEntry([0; 0x1000]);
/// One PDPT. Maps 512GB of memory. Only the first thirty-two entries are used.
#[doc(hidden)]
pub static mut PDPT: PagingEntry = PagingEntry([0; 0x1000]);
/// Thirty-two PDs for the first thirty-two entries in the PDPT. Each PD maps 1GB of memory.
#[doc(hidden)]
pub static mut PDS: [PagingEntry; 32] = [PagingEntry([0; 0x1000]); 32];

// TODO: figure out how to remove these
#[no_mangle]
pub extern "C" fn fmod(a: f64, b: f64) -> f64 {
    libm::fmod(a, b)
}
#[no_mangle]
pub extern "C" fn fmodf(a: f32, b: f32) -> f32 {
    libm::fmodf(a, b)
}
#[no_mangle]
pub extern "C" fn fmin(a: f64, b: f64) -> f64 {
    libm::fmin(a, b)
}
#[no_mangle]
pub extern "C" fn fminf(a: f32, b: f32) -> f32 {
    libm::fminf(a, b)
}
#[no_mangle]
pub extern "C" fn fmax(a: f64, b: f64) -> f64 {
    libm::fmax(a, b)
}
#[no_mangle]
pub extern "C" fn fmaxf(a: f32, b: f32) -> f32 {
    libm::fmaxf(a, b)
}
