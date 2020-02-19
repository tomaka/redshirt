OUTPUT_ARCH("riscv")
ENTRY(_entry_point)

MEMORY {
  RAM : ORIGIN = 0x80000000, LENGTH = 16K
  FLASH : ORIGIN = 0x20400000, LENGTH = 512M
}

SECTIONS {
  .text : {
    _entry_point = .;
    /* Jump to the address of `_start` */
    ASSERT((_start - 0x20400000) < (1 << 21), "_start too far away")
    LONG(
      0x6f |
      ((((_start - 0x20400000) >> 12) & 0xff) << 12) |
      ((((_start - 0x20400000) >> 11) & 0x1) << 20) |
      ((((_start - 0x20400000) >> 1) & 0x3ff) << 21) |
      ((((_start - 0x20400000) >> 20) & 0x1) << 31)
    )

    *(.text)
    *(.text.*)
    *(.rodata)
    *(.rodata.*)
  } > FLASH

  .data : {
    *(.data)
    *(.data.*)
  } > RAM AT > FLASH

  .bss : {
    *(.bss)
    *(.bss.*)
    *(COMMON)
  } > RAM

  /DISCARD/ : {
    *(.eh_frame);
    *(.comment);
  }
}
