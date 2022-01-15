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
//! [`super::entry_point_step3`] Rust function.

#[macro_export]
macro_rules! __gen_boot {
    (
        entry: $entry:path,
        memory_zeroing_start: $memory_zeroing_start:path,
        memory_zeroing_end: $memory_zeroing_end:path,
    ) => {
        const _: () = {
            use core::arch::asm;

            #[naked]
            #[export_name = "_start"]
            unsafe extern "C" fn entry_point_step1() {
                asm!(r#"
                .code32
                    // Disabling interruptions as long as we are not ready to accept them.
                    // This is normally already done by the bootloader, but it costs nothing to
                    // do it here again just in case.
                    cli

                    // Check that we have been loaded by a multiboot2 bootloader.
                    cmp $0x36d76289, %eax
                    jne 5f

                    // Zero the memory requested to be zero'ed.
                    // While the code here is generic, this is typically the BSS segment of the
                    // generated ELF executable. Clearing the BSS segment is normally not
                    // required (it has already been done by the bootloader), but we do it
                    // anyway "just in case".
                    mov ${memory_zeroing_start}, %edi
                    mov ${memory_zeroing_end}, %ecx
                    sub ${memory_zeroing_start}, %ecx
                    jb 5f
                    mov $0, %al
                    cld
                    rep stosb %al, (%edi)

                    // Put the value of EBX in a temporary location, to retrieve it later.
                    // Note that this is done after the memory zero-ing, as it is likely that this
                    // symbol is included in the zero-ing.
                    mov %ebx, ({multiboot_info_ptr})

                    // Check that our CPU supports extended CPUID instructions.
                    mov $0x80000000, %eax
                    cpuid
                    cmp $0x80000001, %eax
                    jb 5f

                    // Check that our CPU supports the features that we need.
                    mov $0x80000001, %eax
                    cpuid
                    test $(1 << 29), %edx     // Test for long mode.
                    jz 5f

                    // Everything is good. CPU is compatible.

                    // Fill the first PML4 entry to point to the PDPT.
                    movl ${pdpt}, %eax
                    or $(1 << 0), %eax    // Present bit. Indicates that the entry is valid.
                    or $(1 << 1), %eax    // Read/write bit. Indicates that the entry is writable.
                    movl %eax, {pml4}

                    // Fill the PDPT entries to point to the PDs.
                    mov $0, %ecx
                2:  mov %ecx, %eax
                    shl $12, %eax           // EAX <- ECX * 4096
                    addl ${pds}, %eax       // EAX <- address of `pds` + ECX * 4096
                    or $(1 << 0), %eax      // Present bit. Indicates that the entry is valid.
                    or $(1 << 1), %eax      // Read/write bit. Indicates that the entry is writable.
                    movl %eax, {pdpt}(, %ecx, 8)      // PDPT[ECX * 8] <- EAX
                    inc %ecx
                    cmp $32, %ecx
                    jne 2b

                    // Fill the PD entries to point to 2MiB pages.
                    mov $0, %ecx
                3:  mov %ecx, %eax
                    shr $12, %eax          // EAX <- ECX >> 12
                    movl %eax, {pds}+4(, %ecx, 8)     // PDs[4 + ECX * 8] <- EAX
                    mov %ecx, %eax         // EAX <- ECX
                    shl $21, %eax          // EAX <- ECX << 21
                    or $(1 << 0), %eax     // Present bit. Indicates that the entry is valid.
                    or $(1 << 1), %eax     // Read/write bit. Indicates that the entry is writable.
                    or $(1 << 7), %eax     // Indicates a 2MiB page.
                    movl %eax, {pds}(, %ecx, 8)       // PDs[ECX * 8] <- EAX
                    inc %ecx
                    cmp $(32 * 512), %ecx
                    jne 3b

                    // Set up the control registers.
                    mov %cr0, %eax
                    and $(~(1 << 2)), %eax          // Clear emulation bit.
                    and $(~(1 << 31)), %eax         // Clear paging bit.
                    movl %eax, %cr0

                    movl ${pml4}, %eax
                    movl %eax, %cr3

                    movl $0, %eax
                    or $(1 << 10), %eax             // Set SIMD floating point exceptions bit.
                    or $(1 << 9), %eax              // Set OSFXSR bit, which enables SIMD.
                    or $(1 << 5), %eax              // Set physical address extension (PAE) bit.
                    movl %eax, %cr4

                    // Set long mode with the EFER bit.
                    movl $0xc0000080, %ecx
                    rdmsr
                    or $(1 << 8), %eax
                    wrmsr

                    // Set up the GDT. It will become active only after we do the `ljmp` below.
                    lgdtl {gdt_ptr}
                
                    // Activate long mode, and jump to the new segment.
                    mov %cr0, %eax
                    or $(1 << 0), %eax              // Set protected mode bit.
                    or $(1 << 1), %eax              // Set co-processor bit.
                    or $(1 << 4), %eax              // Set co-processor extension bit.
                    or $(1 << 31), %eax             // Set paging bit.
                    // The official manual says that instruction right after long mode switch must
                    // be a branch. Tutorials typically don't do that and it might not be strictly
                    // necessary, but to be safe let's follow what the manual says.
                    movl %eax, %cr0

                    ljmp $8, $4f

                .code64
                .align 8
                4:
                    // Set up the stack.
                    movq ${stack} + {stack_size}, %rsp

                    movw $0, %ax
                    movw %ax, %ds
                    movw %ax, %es
                    movw %ax, %fs
                    movw %ax, %gs
                    movw %ax, %ss

                    // Jump to our Rust code.
                    // Pass as parameter the value that the `ebx` register had at initialization,
                    // which is where the multiboot information will be found.
                    mov ({multiboot_info_ptr}), %rdi
                    call {entry_point_step2}
                    cli
                    hlt

                .code32
                // Called if an unrecoverable error happens, such as an incompatible CPU.
                5:
                    movb $'E', 0xb8000
                    movb $0xf, 0xb8001
                    movb $'r', 0xb8002
                    movb $0xf, 0xb8003
                    movb $'r', 0xb8004
                    movb $0xf, 0xb8005
                    movb $'o', 0xb8006
                    movb $0xf, 0xb8007
                    movb $'r', 0xb8008
                    movb $0xf, 0xb8009
                    cli
                    hlt
                "#,
                    entry_point_step2 = sym entry_point_step2,
                    memory_zeroing_start = sym $memory_zeroing_start,
                    memory_zeroing_end = sym $memory_zeroing_end,
                    multiboot_info_ptr = sym $crate::arch::x86_64::boot::MULTIBOOT_INFO_PTR,
                    gdt_ptr = sym $crate::arch::x86_64::gdt::GDT_POINTER,
                    stack = sym $crate::arch::x86_64::boot::MAIN_PROCESSOR_STACK,
                    stack_size = const $crate::arch::x86_64::boot::MAIN_PROCESSOR_STACK_SIZE,
                    pml4 = sym $crate::arch::x86_64::boot::PML4,
                    pdpt = sym $crate::arch::x86_64::boot::PDPT,
                    pds = sym $crate::arch::x86_64::boot::PDS,
                    options(noreturn, att_syntax)); // TODO: convert to Intel syntax
            }

            /// Called by `entry_point_step1` after basic initialization has been performed.
            ///
            /// When this function is called, a stack has been set up and as much memory space as
            /// possible has been identity-mapped (i.e. the virtual memory is equal to the physical
            /// memory).
            ///
            /// Since the kernel was loaded by a multiboot2 bootloader, the first parameter is the
            /// memory address of the multiboot header.
            ///
            /// # Safety
            ///
            /// `multiboot_info` must be a valid memory address that contains valid information.
            ///
            unsafe fn entry_point_step2(multiboot_info: usize) -> ! {
                $crate::arch::x86_64::entry_point_step3(multiboot_info, $entry)
            }
        };
    }
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
