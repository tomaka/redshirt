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

// The code here is a helper for `ap_boot.rs`. This file is intended to be removed, if possible,
// when Rust gets proper support for inline assembly.
//
// This code here, just like any code, is included in the kernel and will be loaded in memory.
// However, it is not actually meant be executed. Instead it is meant to be used as a template.
// Because the associated processor (AP) boot code must be in the first megabyte of memory, we
// first copy this code somewhere in this first megabyte and adjust it.
//
// The `_ap_boot_start` and `_ap_boot_end` symbols encompass the template, so that `ap_boot.rs`
// can copy it. There exist three other symbols `_ap_boot_marker1`, `_ap_boot_marker2` and
// `_ap_boot_marker3` that point to instructions that must be adjusted before execution.
//
// Within this module, we must be careful to not use any absolute address referring to anything
// between `_ap_boot_start` and `_ap_boot_end`, and to not use any relative address referring to
// anything outside of this range, as the addresses will then be wrong when the code gets actually
// executed.

.code16
.align 0x1000
.global _ap_boot_start
.type _ap_boot_start, @function
_ap_boot_start:
    // When we enter here, the CS register is set to the value that we passed through the SIPI,
    // and the IP register is set to `0`.

    movw %cs, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss

    // TODO: properly set up and document the flags here. There's some 1G pages thing that we may not want
    movl $((1 << 10) | (1 << 9) | (1 << 5)), %eax
    movl %eax, %cr4

.global _ap_boot_marker3
_ap_boot_marker3:
    // The `0xff00badd` constant below is replaced with the address of a PML4 table when the
    // template gets adjusted.
    mov $0xff00badd, %edx
    mov %edx, %cr3

    // Enable the EFER.LMA bit, which enables compatibility mode and will make us switch to long
    // mode when we update the CS register.
    mov $0xc0000080, %ecx
    rdmsr
    or $(1 << 8), %eax
    wrmsr

    // Set the appropriate CR0 flags: Paging, Extension Type (math co-processor), and
    // Protected Mode.
    movl $((1 << 31) | (1 << 4) | (1 << 0)), %eax
    movl %eax, %cr0

    // Set up the GDT. Since the absolute address of `_ap_boot_start` is effectively 0 according
    // to the GPU in this 16 bits context, we pass an "absolute" address to `_ap_gdt_ptr` by
    // substracting `_ap_boot_start` from its 32 bits address.
    lgdtl (_ap_gdt_ptr - _ap_boot_start)

.global _ap_boot_marker1
_ap_boot_marker1:
    // A long jump is necessary in order to update the CS registry and properly switch to
    // long mode.
    // The `0xdeaddead` constant below is replaced with the location of `_ap_boot_marker2` when
    // the template gets adjusted.
    ljmpl $8, $0xdeaddead

.code64
.global _ap_boot_marker2
.type _ap_boot_marker2, @function
_ap_boot_marker2:
    // The constants below are replaced with an actual stack location when the template gets
    // adjusted.
    // Set up the stack.
    movq $0x1234567890abcdef, %rsp
    // This is an opaque value for the purpose of this assembly code. It is the parameter that we
    // pass to `ap_after_boot`
    movq $0x9999cccc2222ffff, %rax

    movw $0, %bx
    movw %bx, %ds
    movw %bx, %es
    movw %bx, %fs
    movw %bx, %gs
    movw %bx, %ss

    // In the x86-64 calling convention, the RDI register is used to store the value of the first
    // parameter to pass to a function.
    movq %rax, %rdi

    // We do an indirect call in order to force the assembler to use the absolute address rather
    // than a relative call.
    mov $ap_after_boot, %rdx
    call *%rdx

    cli
    hlt

// Small structure whose location is passed to the CPU in order to load the GDT.
.align 8
_ap_gdt_ptr:
    .short 15
    .long _ap_gdt_table

.global _ap_boot_end
.type _ap_boot_end, @function
_ap_boot_end:
    nop
