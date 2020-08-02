OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH("i386:x86-64")
ENTRY(_start)

MULTIBOOT2_MAGIC = 0xe85250d6;
MULTIBOOT2_ARCH = 0;
MULTIBOOT2_HEADER_LEN = mboot_end - mboot_start;
MULTIBOOT2_CHECKSUM = -(MULTIBOOT2_MAGIC + MULTIBOOT2_ARCH + MULTIBOOT2_HEADER_LEN);

SECTIONS {
  . = 4M;

  .mboot ALIGN(4) : AT(ADDR(.mboot)) {
    mboot_start = .;

    LONG(MULTIBOOT2_MAGIC)
    LONG(MULTIBOOT2_ARCH)
    LONG(MULTIBOOT2_HEADER_LEN)
    LONG(MULTIBOOT2_CHECKSUM)

    /* Flags tag, indicating that we support graphics */
    . = ALIGN(8);
    SHORT(4)
    SHORT(1)    /* flags = 1 means this tag can be ignored if not supported */
    LONG(12)
    LONG(1)     /* 0 = text, 1 = graphics

    /* Framebuffer tag, indicating the preferred graphics mode */
    . = ALIGN(8);
    SHORT(5)
    SHORT(1)    /* flags = 1 means this tag can be ignored if not supported */
    LONG(20)
    LONG(1024)  /* width */
    LONG(768)   /* height */
    LONG(32)    /* depth */

    /* end of tags */
    . = ALIGN(8);
    SHORT(0)
    SHORT(0)
    LONG(8)

    mboot_end = .;
  }

  .text ALIGN(4096) : AT(ADDR(.text)) {
    *(.text)
    *(.text.*)
  }

  .rodata ALIGN(4096) : AT(ADDR(.rodata)) {
    *(.rodata)
    *(.rodata.*)
  }

  .data ALIGN(4096) : AT(ADDR(.data)) {
    *(.data)
    *(.data.*)
  }

  .bss ALIGN(4096) : AT(ADDR(.bss)) {
    __bss_start = .;
    *(.bss)
    *(.bss.*)
    *(COMMON)
    __bss_end = .;
  }

  /DISCARD/ : {
    *(.eh_frame)
    *(.debug_*)
  }
}
