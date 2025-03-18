/*
 * simpleboot.h - Multiboot2 compatible Boot header file.
 * https://codeberg.org/bzt/simpleboot
 *
 * Copyright (C) 2023 bzt, MIT license
 * Copyright (C) 1999,2003,2007,2008,2009,2010  Free Software Foundation, Inc.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to
 * deal in the Software without restriction, including without limitation the
 * rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
 * sell copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.  IN NO EVENT SHALL ANY
 * DEVELOPER OR DISTRIBUTOR BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
 * WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
 * IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 *
 * @brief Simpleboot / Easyboot header file for kernels
 */

#if !defined(SIMPLEBOOT_H) && !defined(EASYBOOT_H)
#define SIMPLEBOOT_H 1

#include <stdint.h>

#ifdef  __cplusplus
extern "C" {
#endif

#define SIMPLEBOOT_MAGIC  "Simpleboot"      /* minimalistic boot loader */
#define EASYBOOT_MAGIC    "Easyboot"        /* fully featured boot manager */

/*** Multiboot2 ***/

/* This should be in the first kernel parameter as well as in %eax. */
#define MULTIBOOT2_BOOTLOADER_MAGIC         0x36d76289

/*  Alignment of multiboot modules. */
#define MULTIBOOT_MOD_ALIGN                 0x00001000

/*  Alignment of the multiboot info structure. */
#define MULTIBOOT_INFO_ALIGN                0x00000008

/*  Flags set in the ’flags’ member of the multiboot header. */
#define MULTIBOOT_TAG_ALIGN                 8
#define MULTIBOOT_TAG_TYPE_END              0
#define MULTIBOOT_TAG_TYPE_CMDLINE          1
#define MULTIBOOT_TAG_TYPE_BOOT_LOADER_NAME 2
#define MULTIBOOT_TAG_TYPE_MODULE           3
#define MULTIBOOT_TAG_TYPE_MMAP             6
#define MULTIBOOT_TAG_TYPE_FRAMEBUFFER      8
#define MULTIBOOT_TAG_TYPE_EFI64            12
#define MULTIBOOT_TAG_TYPE_SMBIOS           13
#define MULTIBOOT_TAG_TYPE_ACPI_OLD         14
#define MULTIBOOT_TAG_TYPE_ACPI_NEW         15
#define MULTIBOOT_TAG_TYPE_EFI64_IH         20
/*  Additional, not in the original Multiboot2 spec. */
#define MULTIBOOT_TAG_TYPE_EDID             256
#define MULTIBOOT_TAG_TYPE_SMP              257
#define MULTIBOOT_TAG_TYPE_PARTUUID         258

/* Multiboot2 information header */
typedef struct {
  uint32_t  total_size;
  uint32_t  reserved;
} multiboot_info_t;

/* common tag header */
typedef struct {
  uint32_t  type;
  uint32_t  size;
} multiboot_tag_t;

/* Boot command line (type 1) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  char      string[0];
} multiboot_tag_cmdline_t;

/* Boot loader name (type 2) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  char      string[0];
} multiboot_tag_loader_t;

/* Modules (type 3) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint32_t  mod_start;
  uint32_t  mod_end;
  char      string[0];
} multiboot_tag_module_t;

/* Memory Map (type 6) */
#define MULTIBOOT_MEMORY_AVAILABLE          1
#define MULTIBOOT_MEMORY_RESERVED           2
/* original EFI type stored in "reserved" field */
#define MULTIBOOT_MEMORY_UEFI               MULTIBOOT_MEMORY_RESERVED
#define MULTIBOOT_MEMORY_ACPI_RECLAIMABLE   3
#define MULTIBOOT_MEMORY_NVS                4
#define MULTIBOOT_MEMORY_BADRAM             5
typedef struct {
  uint64_t  base_addr;
  uint64_t  length;
  uint32_t  type;
  uint32_t  reserved;       /* original EFI Memory Type */
} multiboot_mmap_entry_t;

typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint32_t  entry_size;
  uint32_t  entry_version;
  multiboot_mmap_entry_t entries[0];
} multiboot_tag_mmap_t;

/* Framebuffer info (type 8) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint64_t  framebuffer_addr;
  uint32_t  framebuffer_pitch;
  uint32_t  framebuffer_width;
  uint32_t  framebuffer_height;
  uint8_t   framebuffer_bpp;
  uint8_t   framebuffer_type; /* must be 1 */
  uint16_t  reserved;
  uint8_t   framebuffer_red_field_position;
  uint8_t   framebuffer_red_mask_size;
  uint8_t   framebuffer_green_field_position;
  uint8_t   framebuffer_green_mask_size;
  uint8_t   framebuffer_blue_field_position;
  uint8_t   framebuffer_blue_mask_size;
} multiboot_tag_framebuffer_t;

/* EFI 64-bit image handle pointer (type 12) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint64_t  pointer;
} multiboot_tag_efi64_t;

/* SMBIOS tables (type 13) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint8_t   major;
  uint8_t   minor;
  uint8_t   reserved[6];
  uint8_t   tables[0];
} multiboot_tag_smbios_t;

/* ACPI old RSDP (type 14) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint8_t   rsdp[0];
} multiboot_tag_old_acpi_t;

/* ACPI new RSDP (type 15) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint8_t   rsdp[0];
} multiboot_tag_new_acpi_t;

/* EFI 64-bit image handle pointer (type 20) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint64_t  pointer;
} multiboot_tag_efi64_ih_t;

/* EDID supported monitor resolutions (type 256) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint8_t   edid[0];
} multiboot_tag_edid_t;

/* SMP supported (type 257) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint32_t  numcores;
  uint32_t  running;
  uint32_t  bspid;
} multiboot_tag_smp_t;

/* Partition UUIDs (type 258) */
typedef struct {
  uint32_t  type;
  uint32_t  size;
  uint8_t   bootuuid[16];
  uint8_t   rootuuid[16];
} multiboot_tag_partuuid_t;

#ifdef  __cplusplus
}
#endif

#endif /* SIMPLEBOOT_H */
