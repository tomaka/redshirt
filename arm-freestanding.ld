ENTRY(_start)

SECTIONS {
    . = 0x0;
    .startup : AT(ADDR(.startup)) {
        /* Jump to the entry point of the kernel */
        /* Note that the value is an offset, therefore this only works because our address here is 0x0 */
        LONG(0xea000000 | (((0x8000 >> 2) - 2) & 0x1FFFFFF))

        /* Interrupt descriptor table */
        LONG(0xeafffffe)
        LONG(0xeafffffe)
        LONG(0xeafffffe)
        LONG(0xeafffffe)
        LONG(0xeafffffe)
        LONG(0xeafffffe)
        LONG(0xeafffffe)
    }

    . = 0x8000;
    .text : AT(ADDR(.text)) {
        _entry_point = .;
        /* Same as above, jump to the address of `_start` */
        /* TODO: this always fails, no clue why: ASSERT(((_start >> 2) - 2) == (((_start >> 2) - 2) & 0x1FFFFFF), "_start too far away") */
        LONG(0xea000000 | ((((_start - 0x8000) >> 2) - 2) & 0x1FFFFFF))

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
