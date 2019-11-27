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
    test $(1 << 29), %edx     // test for long mode
    jz .print_err_and_stop
    test $(1 << 26), %edx     // test for 1G pages
    //jz .print_err_and_stop        // TODO: QEMU reports that it doesn't support this, but works anyway

    // Everything is good. CPU is compatible.

    // Fill the PML4 and PDPT to identity-map the first 512GiB.
    movl $pdpt, %eax
    or $((1 << 1) | (1 << 0)), %eax
    movl %eax, pml4
    mov $0, %ecx
L0: mov %ecx, %eax
    shl $30, %eax       // FIXME: oooooops, this doesn't fit eax, doh
    or $((1 << 7) | (1 << 1) | (1 << 0)), %eax
    movl %eax, pdpt(, %ecx, 8)
    inc %ecx
    cmp $512, %ecx
    jne L0

    // Set up the control registers.
    mov %cr0, %eax
    and $(~(1 << 2)), %eax
    or $(1 << 1), %eax
    and $(~(1 << 31)), %eax
    movl %eax, %cr0

    movl $pml4, %eax
    movl %eax, %cr3

    movl $((1 << 10) | (1 << 9) | (1 << 5)), %eax
    movl %eax, %cr4

    movl $0xc0000080, %ecx
    rdmsr
    or $(1 << 8), %eax
    wrmsr

    movl $((1 << 31) | (1 << 4) | (1 << 0)), %eax
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

    // Jump to our Rust code
    // Pass as parameter the content of `multiboot_info_ptr`
    mov multiboot_info_ptr, %rdi
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

// This is our GDT. It is necessary to have one in order to jump to long mode.
// It contains two entries: one null entry (mandatory), and one for our kernel.
.align 0x1000
gdt_table:
    .8byte 0
    .8byte (1 << 53) | (1 << 47) | (1 << 44) | (1 << 43)
    .skip (0x800 - (2 * 8))

// Small structure whose location is passed to the CPU.
.align 8
gdt_ptr:
    .short 0x800 - 1
    .long gdt_table



.section .bss
// PML4. The entry point for our paging system.
.comm pml4, 0x1000, 0x1000
// A single PDPT. Only the first entry of the PML4 will be set and points to this PDPT.
.comm pdpt, 0x1000, 0x1000

// Stack used by the kernel.
.comm stack, KERNEL_STACK_SIZE, 0x10000

// Small variable used to store the value of ebx passed by the bootloader.
.comm multiboot_info_ptr, 4, 8
