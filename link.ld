OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH("i386:x86-64")
ENTRY(_start)

MULTIBOOT2_MAGIC = 0xe85250d6;
MULTIBOOT2_ARCH = 0;
MULTIBOOT2_HEADER_LEN = mboot_end - mboot_start;
MULTIBOOT2_CHECKSUM = -(MULTIBOOT2_MAGIC + MULTIBOOT2_ARCH + MULTIBOOT2_HEADER_LEN);

SECTIONS
{
  . = 100000;
  kernel_start = .;

  .mboot ALIGN(32) : AT(ADDR(.mboot)) {
    mboot_start = .;

    LONG(MULTIBOOT2_MAGIC)
    LONG(MULTIBOOT2_ARCH)
    LONG(MULTIBOOT2_HEADER_LEN)
    LONG(MULTIBOOT2_CHECKSUM)

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
    bss_start = .;
    *(.bss)
    *(.bss.*)
    *(COMMON)
  }
}
