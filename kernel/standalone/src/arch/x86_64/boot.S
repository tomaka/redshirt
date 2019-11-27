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

#define KERNEL_STACK_SIZE 0x800000

.section .text
.code32
.global _start
.type _start, @function
_start:
    // See if we have been loaded by a multiboot2 bootloader
    cmp $0x36d76289, %eax
    jne L1
    mov %ebx, multiboot_info_ptr
L1:

    // TODO: check for long mode support in CPUID
    // TODO: check for 1g pages support in the CPUID
    // TODO: check for sse/sse2 support in the CPUID

    // Fill the PML4 and PDPT to identity-map the first 512GiB
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

    // Set up the control registers
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

    // Set up the GDT to allow us to jump to long mode
    lgdt gdt_ptr
    ljmp $8, $_start64

.code64
.global _start64
.type _start64, @function
_start64:
    // Set up a stack
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
    call loader_main
    cli
    hlt


.section .rodata

.align 0x1000
gdt_table:
    .8byte 0
    .8byte (1 << 53) | (1 << 47) | (1 << 44) | (1 << 43)
    .skip (0x800 - (2 * 8))

.align 8
gdt_ptr:
     .short 0x800 - 1
     .long gdt_table



.section .bss
.comm pml4, 0x1000, 0x1000
.comm pdpt, 0x1000, 0x1000

.comm stack, KERNEL_STACK_SIZE, 0x10000

.comm multiboot_info_ptr, 4, 8
