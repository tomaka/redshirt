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
// instead first copy it somewhere in this first megabyte and adjust it.
//
// The `_ap_boot_start` and `_ap_boot_end` symbols encompass this template, so that `ap_boot.rs`
// can copy it. There exist three other symbols `_ap_boot_marker1`, `_ap_boot_marker2` and
// `_ap_boot_marker3` that point to instructions that must be adjusted before execution.
//
// Within this module, we must be careful to not use any absolute address referring to anything
// between `_ap_boot_start` and `_ap_boot_end`, as the address will then be wrong when actually
// executed.

.code16
.global _ap_boot_start
.type _ap_boot_start, @function
_ap_boot_start:
    // When we enter here, the `%cs` register is set to the value that we passed through the
    // SIPI, and the `%ip` register is set to `0`.

    xor %eax, %eax
    movw %cs, %ax
    movw %ax, %ds
    shll $4, %eax

    // `%eax` now contains the address of `_ap_boot_start`.
    // We use this information to store the location of `_ap_gdt_table` at the appropriate location in
    // `_ap_gdt_ptr`.
    addl $0 + _ap_gdt_table - _ap_boot_start, %eax
    movl %eax, (_ap_gdt_ptr - _ap_boot_start + 2)

    movl $((1 << 10) | (1 << 9) | (1 << 5)), %eax
    movl %eax, %cr4

.global _ap_boot_marker3
_ap_boot_marker3:
    mov $0xff00badd, %edx
    mov %edx, %cr3

    mov $0xc0000080, %ecx
    rdmsr
    or $(1 << 8), %eax
    wrmsr

    mov %cr0, %eax
    and $(~(1 << 2)), %eax
    or $(1 << 1), %eax
    and $(~(1 << 31)), %eax
    movl %eax, %cr0

    // Set the GDT and do a long jump.
    lgdtl (_ap_gdt_ptr - _ap_boot_start)
.global _ap_boot_marker1
_ap_boot_marker1:
    ljmpl $8, $0xdeaddead

.code64
.global _ap_boot_marker2
.type _ap_boot_marker2, @function
_ap_boot_marker2:
    // The constants here will be replaced with an actual stack location when the template
    // gets adjusted.
    // Set up the stack.
    movq $0x1234567890abcdef, %rsp
    movq $0x9999cccc2222ffff, %rax

    movw $0, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss

    call ap_after_boot
    cli
    hlt



// TODO: reuse the code from boot.S?
// TODO: build the GDT from ap_boot.rs?


// This is our GDT. It is necessary to have one in order to jump to long mode.
// It contains two entries: one null entry (mandatory), and one for our kernel.
.align 0x1000
_ap_gdt_table:
    .8byte 0
    .8byte (1 << 53) | (1 << 47) | (1 << 44) | (1 << 43)
    .skip (0x800 - (2 * 8))

// Small structure whose location is passed to the CPU.
.align 8
_ap_gdt_ptr:
    .short 0x800 - 1
    .long 0


.global _ap_boot_end
.type _ap_boot_end, @function
_ap_boot_end:
    nop
