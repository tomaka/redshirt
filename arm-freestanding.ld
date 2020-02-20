ENTRY(_entry_point)

SECTIONS {
    . = 0x10000;
    .text : AT(ADDR(.text)) {
        _entry_point = .;
        /* Same as above, jump to the address of `_start` */
        /* TODO: this always fails, no clue why: ASSERT((((_start - 0x10000) >> 2) - 2) == (((_start >> 2) - 2) & 0x1FFFFFF), "_start too far away") */
        LONG(0xea000000 | ((((_start - 0x10000) >> 2) - 2) & 0x1FFFFFF))

        *(.text*)
        *(.rodata*)
    }

    /* Garbage that the compiler seems to introduce for stack unwinding purposes */
    .ARM.exidx : AT(ADDR(.ARM.exidx)) {
        *(.ARM.exidx*)
        *(.gnu.linkonce.armexidx.*)
    }

    .data : AT(ADDR(.data)) {
        *(.data*)
    }

    .bss : AT(ADDR(.bss)) {
        *(.bss*)
        *(COMMON*)
    }
}
