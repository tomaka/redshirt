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

// This file contains the entry point of our kernel.
//
// Once the bootloader finishes loading the kernel (as an ELF file), it will run its entry point,
// which is the `_start` function defined in this file.
//
// Since we are conforming to the multiboot2 specifications, the bootloader is expected to set the
// ebx register to the memory address of a data structure containing information about the
// environment.
//
// The environment in which we start in is the protected mode where the kernel is identity-mapped.
//
// The role of the `_start` function below is to perform some checks, set up everything that is
// needed to run freestanding 64bits Rust code (i.e. a stack, paging, long mode), and call the
// `after_boot` Rust function.

#define KERNEL_STACK_SIZE 0x800000

.section .text
.code32
.global _start
.type _start, @function
_start:
    // Check that we have been loaded by a multiboot2 bootloader.
    cmp $0x36d76289, %eax
    jne .print_err_and_stop
    mov %ebx, multiboot_info_ptr

    // Check that our CPU supports extended CPUID instructions.
    mov $0x80000000, %eax
    cpuid
    cmp $0x80000001, %eax
    jb .print_err_and_stop

    // Check that our CPU supports the features that we need.
    mov $0x80000001, %eax
    cpuid
    test $(1 << 29), %edx     // Test for long mode.
    jz .print_err_and_stop

    // Everything is good. CPU is compatible.

    // Fill the first PML4 entry to point to the PDPT.
    movl $pdpt, %eax
    or $(1 << 0), %eax      // Present bit. Indicates that the entry is valid.
    or $(1 << 1), %eax      // Read/write bit. Indicates that the entry is writable.
    movl %eax, pml4

    // Fill the PDPT entries to point to the PDs.
    mov $0, %ecx
L0: mov %ecx, %eax
    shl $12, %eax                   // EAX <- ECX * 4096
    addl $pds, %eax                 // EAX <- address of `pds` + ECX * 4096
    or $(1 << 0), %eax              // Present bit. Indicates that the entry is valid.
    or $(1 << 1), %eax              // Read/write bit. Indicates that the entry is writable.
    movl %eax, pdpt(, %ecx, 8)      // PDPT[ECX * 8] <- EAX
    inc %ecx
    cmp $4, %ecx
    jne L0

    // Fill the PD entries to point to 2MiB pages.
    mov $0, %ecx
L1: mov %ecx, %eax
    shr $12, %eax                   // EAX <- ECX >> 12
    movl %eax, pds+4(, %ecx, 8)     // PDs[4 + ECX * 8] <- EAX
    mov %ecx, %eax                  // EAX <- ECX
    shl $21, %eax                   // EAX <- ECX << 21
    or $(1 << 0), %eax              // Present bit. Indicates that the entry is valid.
    or $(1 << 1), %eax              // Read/write bit. Indicates that the entry is writable.
    or $(1 << 7), %eax              // Indicates a 2MiB page.
    movl %eax, pds(, %ecx, 8)       // PDs[ECX * 8] <- EAX
    inc %ecx
    cmp $(4 * 512), %ecx
    jne L1

    // Set up the control registers.
    mov %cr0, %eax
    and $(~(1 << 2)), %eax          // Clear emulation bit.
    and $(~(1 << 31)), %eax         // Clear paging bit.
    movl %eax, %cr0

    movl $pml4, %eax
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

    mov %cr0, %eax
    or $(1 << 0), %eax              // Set protected mode bit.
    or $(1 << 1), %eax              // Set co-processor bit.
    or $(1 << 4), %eax              // Set co-processor extension bit.
    or $(1 << 31), %eax             // Set paging bit.
    movl %eax, %cr0
    // TODO: official manual says that instruction right after long mode switch must be a branch?!?!?!

    // Set up the GDT to allow us to jump to long mode.
    lgdt gdt_ptr
    ljmp $8, $_start64

.code64
.global _start64
.type _start64, @function
_start64:
    // Set up the stack.
    movq $stack + KERNEL_STACK_SIZE, %rsp

    movw $0, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss

    // Since `multiboot_info_ptr` is in the BSS segment, we move out its value before clearing
    // the BSS.
    //xor %rdx, %rdx
    //mov multiboot_info_ptr, %edx

    // Clear the BSS segment.
    // While this is normally not required, we do it anyway "just in case".
    // This implementation is very slow, but we don't care as it's just a one time operation.
    //mov __bss_start, %rdi
L3: //cmp __bss_end, %rdi
    //jmp L2//jae L2
    //movb $0, (%rdi)
    //inc %rdi
    //jmp L3
L2: //nop

    // Jump to our Rust code
    // Pass as parameter the content of `multiboot_info_ptr`
    mov multiboot_info_ptr, %rdi
    //mov %rdx, %rdi
    call after_boot
    cli
    hlt

.code32
// Called if an unrecoverable error happens, such as an incompatible CPU.
.print_err_and_stop:
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

.section .rodata

// Small structure whose location is passed to the CPU.
.align 8
gdt_ptr:
    .short 15
    .long gdt_table


.section .bss
// PML4. The entry point for our paging system.
.comm pml4, 0x1000, 0x1000
// One PDPT. Maps 512GB of memory. Only the first four entries are used.
.comm pdpt, 0x1000, 0x1000
// Four PDs for the first four entries in the PDPT. Each PD maps 1GB of memory.
// TODO: how can we be sure that mapping 4GiB is enough, and that the kernel doesn't go above?
.comm pds, 4 * 0x1000, 0x1000

// Stack used by the kernel.
.comm stack, KERNEL_STACK_SIZE, 0x8

// Small variable used to store the value of ebx passed by the bootloader.
.comm multiboot_info_ptr, 4, 8
