/*
 * src/loader.h
 * https://gitlab.com/bztsrc/simpleboot
 *
 * Copyright (C) 2023 bzt (bztsrc@gitlab), MIT license
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
 * @brief Defines for the Simpleboot loader
 */

#ifndef NULL
#define NULL (void*)0
#endif

#define FB_COLOR(r,g,b) (((vidmode.framebuffer_red_mask_size > 8 ? (r) << (vidmode.framebuffer_red_mask_size - 8) : \
                          (r) >> (8 - vidmode.framebuffer_red_mask_size)) << vidmode.framebuffer_red_field_position) |\
                         ((vidmode.framebuffer_green_mask_size > 8 ? (g) << (vidmode.framebuffer_green_mask_size - 8) : \
                          (g) >> (8 - vidmode.framebuffer_green_mask_size)) << vidmode.framebuffer_green_field_position) |\
                         ((vidmode.framebuffer_blue_mask_size > 8 ? (b) << (vidmode.framebuffer_blue_mask_size - 8) : \
                          (b) >> (8 - vidmode.framebuffer_blue_mask_size)) << vidmode.framebuffer_blue_field_position))

enum { MODE_UNKNOWN, MODE_VBR, MODE_PE32, MODE_MB32, MODE_MB64, MODE_LIN };

/*** ELF defines and structs ***/

#define ELFMAG      "\177ELF"
#define SELFMAG     4
#define EI_CLASS    4       /* File class byte index */
#define ELFCLASS32  1       /* 32-bit objects */
#define ELFCLASS64  2       /* 64-bit objects */
#define EI_DATA     5       /* Data encoding byte index */
#define ELFDATA2LSB 1       /* 2's complement, little endian */
#define PT_LOAD     1       /* Loadable program segment */
#define PT_DYNAMIC  2       /* Dynamic linking information */
#define PT_INTERP   3       /* Program interpreter */
#define PF_X (1 << 0)       /* Segment is executable */
#define PF_W (1 << 1)       /* Segment is writable */
#define PF_R (1 << 2)       /* Segment is readable */
#define EM_386      3       /* Intel 80386 */
#define EM_X86_64   62      /* AMD x86_64 architecture */
#define EM_AARCH64  183     /* ARM aarch64 architecture */
#define EM_RISCV    243     /* RISC-V riscv64 architecture */

typedef struct {
  unsigned char e_ident[16];/* Magic number and other info */
  uint16_t  e_type;         /* Object file type */
  uint16_t  e_machine;      /* Architecture */
  uint32_t  e_version;      /* Object file version */
  uint32_t  e_entry;        /* Entry point virtual address */
  uint32_t  e_phoff;        /* Program header table file offset */
  uint32_t  e_shoff;        /* Section header table file offset */
  uint32_t  e_flags;        /* Processor-specific flags */
  uint16_t  e_ehsize;       /* ELF header size in bytes */
  uint16_t  e_phentsize;    /* Program header table entry size */
  uint16_t  e_phnum;        /* Program header table entry count */
  uint16_t  e_shentsize;    /* Section header table entry size */
  uint16_t  e_shnum;        /* Section header table entry count */
  uint16_t  e_shstrndx;     /* Section header string table index */
} __attribute__((packed)) Elf32_Ehdr;

typedef struct {
  unsigned char e_ident[16];/* Magic number and other info */
  uint16_t  e_type;         /* Object file type */
  uint16_t  e_machine;      /* Architecture */
  uint32_t  e_version;      /* Object file version */
  uint64_t  e_entry;        /* Entry point virtual address */
  uint64_t  e_phoff;        /* Program header table file offset */
  uint64_t  e_shoff;        /* Section header table file offset */
  uint32_t  e_flags;        /* Processor-specific flags */
  uint16_t  e_ehsize;       /* ELF header size in bytes */
  uint16_t  e_phentsize;    /* Program header table entry size */
  uint16_t  e_phnum;        /* Program header table entry count */
  uint16_t  e_shentsize;    /* Section header table entry size */
  uint16_t  e_shnum;        /* Section header table entry count */
  uint16_t  e_shstrndx;     /* Section header string table index */
} __attribute__((packed)) Elf64_Ehdr;

typedef struct {
  uint32_t  p_type;         /* Segment type */
  uint32_t  p_offset;       /* Segment file offset */
  uint32_t  p_vaddr;        /* Segment virtual address */
  uint32_t  p_paddr;        /* Segment physical address */
  uint32_t  p_filesz;       /* Segment size in file */
  uint32_t  p_memsz;        /* Segment size in memory */
  uint32_t  p_align;        /* Segment alignment */
} __attribute__((packed)) Elf32_Phdr;

typedef struct {
  uint32_t  p_type;         /* Segment type */
  uint32_t  p_flags;        /* Segment flags */
  uint64_t  p_offset;       /* Segment file offset */
  uint64_t  p_vaddr;        /* Segment virtual address */
  uint64_t  p_paddr;        /* Segment physical address */
  uint64_t  p_filesz;       /* Segment size in file */
  uint64_t  p_memsz;        /* Segment size in memory */
  uint64_t  p_align;        /* Segment alignment */
} __attribute__((packed)) Elf64_Phdr;

/*** PE32+ defines and structs ***/

#define MZ_MAGIC                    0x5a4d      /* "MZ" */
#define PE_MAGIC                    0x00004550  /* "PE\0\0" */
#define IMAGE_FILE_MACHINE_I386     0x014c      /* Intel 386 architecture */
#define IMAGE_FILE_MACHINE_AMD64    0x8664      /* AMD x86_64 architecture */
#define IMAGE_FILE_MACHINE_ARM64    0xaa64      /* ARM aarch64 architecture */
#define IMAGE_FILE_MACHINE_RISCV64  0x5064      /* RISC-V riscv64 architecture */
#define PE_OPT_MAGIC_PE32PLUS       0x020b      /* PE32+ format */

typedef struct {
  uint16_t  magic;        /* MZ magic */
  uint16_t  reserved[29]; /* reserved */
  uint32_t  peaddr;       /* address of pe header */
} __attribute__((packed)) mz_hdr;

typedef struct {
  uint32_t  magic;        /* PE magic */
  uint16_t  machine;      /* machine type */
  uint16_t  sections;     /* number of sections */
  uint32_t  timestamp;    /* time_t */
  uint32_t  sym_table;    /* symbol table offset */
  uint32_t  numsym;       /* number of symbols */
  uint16_t  opt_hdr_size; /* size of optional header */
  uint16_t  flags;        /* flags */
  uint16_t  file_type;    /* file type, PE32PLUS magic */
  uint8_t   ld_major;     /* linker major version */
  uint8_t   ld_minor;     /* linker minor version */
  uint32_t  text_size;    /* size of text section(s) */
  uint32_t  data_size;    /* size of data section(s) */
  uint32_t  bss_size;     /* size of bss section(s) */
  uint32_t  entry_point;  /* file offset of entry point */
  uint32_t  code_base;    /* relative code addr in ram */
  union {
    struct {
      uint32_t data_base; /* the preferred load address */
      uint32_t img_base;  /* the preferred load address */
    } pe32;
    struct {
      uint64_t img_base;  /* the preferred load address */
    } pe64;
  } data;
} __attribute__((packed)) pe_hdr;

typedef struct {
  char      name[8];      /* section name */
  uint32_t  vsiz;         /* virtual size */
  uint32_t  vaddr;        /* virtual address */
  uint32_t  rsiz;         /* size of raw data */
  uint32_t  raddr;        /* pointer to raw data */
  uint32_t  reloc;        /* pointer to relocations */
  uint32_t  ln;           /* pointer to line numbers */
  uint16_t  nreloc;       /* number of relocations */
  uint16_t  nln;          /* number of line numbers */
  uint32_t  chr;          /* characteristics */
} __attribute__((packed)) pe_sec;

/*** Linux kernel header ***/
#define HDRSMAG "HdrS"
#define E820_MAX_ENTRIES_ZEROPAGE 128
#define LOADED_HIGH   (1<<0)
#define CAN_USE_HEAP  (1<<7)
#define XLF_KERNEL_64 (1<<0)
/* holy fucking shit, these go to orig_video_isVGA... undocumented, I had to grep the kernel source to figure out */
#define VIDEO_TYPE_VLFB 0x23  /* VESA VGA in graphic mode	*/
#define VIDEO_TYPE_EFI  0x70  /* EFI graphic mode */
/* for hdr.vid_mode */
#define VIDEO_MODE_ASK  0xfffd
#define VIDEO_MODE_CUR  0x0f04

typedef struct {
  uint8_t   setup_sects;      /* if 0, WR 4 */
  uint16_t  root_flags;
  uint32_t  syssize;
  uint16_t  ram_size;         /* DO NOT USE */
  uint16_t  vid_mode;         /* WR */
  uint16_t  root_dev;
  uint16_t  boot_flag;        /* 0xAA55 magic */
  /* 0x200 */
  uint16_t  jump;
  uint32_t  header;           /* magic "HdrS" */
  uint16_t  version;          /* boot protocol version, 0x20c minimum */
  uint32_t  realmode_swtch;
  uint16_t  start_sys_seg;    /* obsolete, 0x1000 */
  uint16_t  kernel_version;   /* pointer to kernel version string */
  uint8_t   type_of_loader;   /* WR 0x14 */
  uint8_t   loadflags;        /* WR, bit 1: LOADED_HIGH, bit 7: heap_end_ptr is valid */
  uint16_t  setup_move_size;
  uint32_t  code32_start;
  uint32_t  ramdisk_image;    /* WR, initrd load address (set by boot loader) */
  uint32_t  ramdisk_size;     /* WR, initrd size (set by boot loader) */
  uint32_t  bootsect_klundge; /* DO NOT USE */
  uint16_t  heap_end_ptr;     /* WR */
  uint8_t   ext_loader_ver;
  uint8_t   ext_loader_type;
  uint32_t  cmd_line_ptr;     /* WR */
  uint32_t  initrd_addr_max;
  /* we might not have these, check version */
  uint32_t  kernel_alignment;
  uint8_t   reloc_kernel;
  uint8_t   min_alignment;
  uint16_t  xloadflags;
  uint32_t  cmdline_size;
  uint32_t  hw_subarch;       /* WR 0 */
  uint64_t  hw_subarch_data;  /* WR 0 */
  uint32_t  payload_offset;
  uint32_t  payload_length;
  uint64_t  setup_data;
  uint64_t  pref_address;     /* prefered loading address */
  uint32_t  init_size;
  uint32_t  handover_offset;  /* EFI entry point, obsolete */
  uint32_t  kernel_info_offset; /* v2.15+ WR 0 */
} __attribute__((packed)) linux_boot_t;

typedef struct {
  uint64_t  addr;
  uint64_t  size;
  uint32_t  type;
} __attribute__((packed)) linux_e820_entry_t;

/* The so-called "zeropage" */
typedef struct {
  /* screen info                0x000 */
  uint8_t   orig_x;           /* 0x00 */
  uint8_t   orig_y;           /* 0x01 */
  uint16_t  ext_mem_k;        /* 0x02 */
  uint16_t  orig_video_page;  /* 0x04 */
  uint8_t   orig_video_mode;  /* 0x06 */
  uint8_t   orig_video_cols;  /* 0x07 */
  uint8_t   flags;            /* 0x08 */
  uint8_t   unused2;          /* 0x09 */
  uint16_t  orig_video_ega_bx;/* 0x0a */
  uint16_t  unused3;          /* 0x0c */
  uint8_t   orig_video_lines; /* 0x0e */
  uint8_t   orig_video_isVGA; /* 0x0f */
  uint16_t  orig_video_points;/* 0x10 */
  /* VESA graphic mode -- linear frame buffer */
  uint16_t  lfb_width;        /* 0x12 */
  uint16_t  lfb_height;       /* 0x14 */
  uint16_t  lfb_depth;        /* 0x16 */
  uint32_t  lfb_base;         /* 0x18 */
  uint32_t  lfb_size;         /* 0x1c */
  uint16_t  cl_magic, cl_offset; /* 0x20 */
  uint16_t  lfb_linelength;   /* 0x24 */
  uint8_t   red_size;         /* 0x26 */
  uint8_t   red_pos;          /* 0x27 */
  uint8_t   green_size;       /* 0x28 */
  uint8_t   green_pos;        /* 0x29 */
  uint8_t   blue_size;        /* 0x2a */
  uint8_t   blue_pos;         /* 0x2b */
  uint8_t   rsvd_size;        /* 0x2c */
  uint8_t   rsvd_pos;         /* 0x2d */
  uint16_t  vesapm_seg;       /* 0x2e */
  uint16_t  vesapm_off;       /* 0x30 */
  uint16_t  pages;            /* 0x32 */
  uint16_t  vesa_attributes;  /* 0x34 */
  uint32_t  capabilities;     /* 0x36 */
  uint32_t  ext_lfb_base;     /* 0x3a */
  uint8_t   _reserved[2];     /* 0x3e */
  uint8_t   apm_bios_info[0x054 - 0x040]; /* 0x040 */
  uint8_t   _pad2[4];         /* 0x054 */
  uint64_t  tboot_addr;       /* 0x058 */
  uint8_t   ist_info[0x070 - 0x060]; /* 0x060 */
  uint64_t  acpi_rsdp_addr;   /* 0x070 */
  uint8_t   _pad3[8];         /* 0x078 */
  uint8_t   hd0_info[16]; /* obsolete! */ /* 0x080 */
  uint8_t   hd1_info[16]; /* obsolete! */ /* 0x090 */
  /* sys_desc_table, obsolete    0x0a0 */
  uint16_t  length;
  uint8_t   table[14];
  /* olpc_ofw_header             0x0b0 */
  uint32_t  ofw_magic;
  uint32_t  ofw_version;
  uint32_t  cif_handler;
  uint32_t  irq_desc_table;
  uint32_t  ext_ramdisk_image;/* 0x0c0 */
  uint32_t  ext_ramdisk_size; /* 0x0c4 */
  uint32_t  ext_cmd_line_ptr; /* 0x0c8 */
  uint8_t   _pad4[112];       /* 0x0cc */
  uint32_t  cc_blob_address;  /* 0x13c */
  uint8_t   edid_info[0x1c0 - 0x140]; /* 0x140 */
  /* efi_info                    0x1c0 */
  uint32_t  efi_loader_signature;
  uint32_t  efi_systab;
  uint32_t  efi_memdesc_size;
  uint32_t  efi_memdesc_version;
  uint32_t  efi_memmap;
  uint32_t  efi_memmap_size;
  uint32_t  efi_systab_hi;
  uint32_t  efi_memmap_hi;
  uint32_t  alt_mem_k;        /* 0x1e0 */
  uint32_t  scratch;    /* Scratch field! */  /* 0x1e4 */
  uint8_t   e820_entries;     /* 0x1e8 */
  uint8_t   eddbuf_entries;   /* 0x1e9 */
  uint8_t   edd_mbr_sig_buf_entries; /* 0x1ea */
  uint8_t   kbd_status;       /* 0x1eb */
  uint8_t   secure_boot;      /* 0x1ec */
  uint8_t   _pad5[2];         /* 0x1ed */
  uint8_t   sentinel;         /* 0x1ef */
  uint8_t   _pad6[1];         /* 0x1f0 */
  linux_boot_t hdr;    /* setup header */  /* 0x1f1 */
  uint8_t   _pad7[0x290-0x1f1-sizeof(linux_boot_t)];
  uint8_t   edd_mbr_sig_buffer[0x2d0 - 0x290];  /* 0x290 */
  linux_e820_entry_t e820_table[E820_MAX_ENTRIES_ZEROPAGE]; /* 0x2d0 */
  uint8_t   _pad8[48];        /* 0xcd0 */
  uint8_t   eddbuf[0xeec - 0xd00]; /* 0xd00 */
  uint8_t   _pad9[276];       /* 0xeec */
} __attribute__((packed)) linux_boot_params_t;

/*** EFI ***/

#ifndef EFIAPI
# ifdef _MSC_EXTENSIONS
#  define EFIAPI __cdecl
# else
#  ifdef __x86_64__
#   define EFIAPI __attribute__((ms_abi))
#  else
#   define EFIAPI
#  endif
# endif
#endif
#define EFI_ERROR(a)           (((intn_t) a) < 0)
#define EFI_SUCCESS                            0
#define EFI_LOAD_ERROR        0x8000000000000001
#define EFI_BUFFER_TOO_SMALL  0x8000000000000005

typedef void     *efi_handle_t;
typedef void     *efi_event_t;
typedef int64_t  intn_t;
typedef uint8_t  boolean_t;
typedef uint64_t uintn_t;
typedef uint64_t efi_status_t;
typedef uint64_t efi_physical_address_t;
typedef uint64_t efi_virtual_address_t;

typedef enum {
    AllocateAnyPages,
    AllocateMaxAddress,
    AllocateAddress,
    MaxAllocateType
} efi_allocate_type_t;

typedef enum {
    EfiReservedMemoryType,
    EfiLoaderCode,
    EfiLoaderData,
    EfiBootServicesCode,
    EfiBootServicesData,
    EfiRuntimeServicesCode,
    EfiRuntimeServicesData,
    EfiConventionalMemory,
    EfiUnusableMemory,
    EfiACPIReclaimMemory,
    EfiACPIMemoryNVS,
    EfiMemoryMappedIO,
    EfiMemoryMappedIOPortSpace,
    EfiPalCode,
    EfiPersistentMemory,
    EfiUnacceptedMemoryType,
    EfiMaxMemoryType
} efi_memory_type_t;

typedef struct {
  uint32_t  Type;
  uint32_t  Pad;
  uint64_t  PhysicalStart;
  uint64_t  VirtualStart;
  uint64_t  NumberOfPages;
  uint64_t  Attribute;
} efi_memory_descriptor_t;
#define NextMemoryDescriptor(Ptr,Size)  ((efi_memory_descriptor_t *) (((uint8_t *) Ptr) + Size))

typedef enum {
    AllHandles,
    ByRegisterNotify,
    ByProtocol
} efi_locate_search_type_t;

typedef struct {
    uint64_t    Signature;
    uint32_t    Revision;
    uint32_t    HeaderSize;
    uint32_t    CRC32;
    uint32_t    Reserved;
} efi_table_header_t;

typedef struct {
    uint16_t    Year;       /* 1998 - 2XXX */
    uint8_t     Month;      /* 1 - 12 */
    uint8_t     Day;        /* 1 - 31 */
    uint8_t     Hour;       /* 0 - 23 */
    uint8_t     Minute;     /* 0 - 59 */
    uint8_t     Second;     /* 0 - 59 */
    uint8_t     Pad1;
    uint32_t    Nanosecond; /* 0 - 999,999,999 */
    int16_t     TimeZone;   /* -1440 to 1440 or 2047 */
    uint8_t     Daylight;
    uint8_t     Pad2;
} efi_time_t;

typedef struct {
  uint32_t  Data1;
  uint16_t  Data2;
  uint16_t  Data3;
  uint8_t   Data4[8];
} __attribute__((packed)) guid_t;


/* Simple text input / output interface */
typedef struct {
    uint16_t    ScanCode;
    uint16_t    UnicodeChar;
} efi_input_key_t;

typedef efi_status_t (EFIAPI *efi_input_reset_t)(void *This, boolean_t ExtendedVerification);
typedef efi_status_t (EFIAPI *efi_input_read_key_t)(void *This, efi_input_key_t *Key);

typedef struct {
    efi_input_reset_t           Reset;
    efi_input_read_key_t        ReadKeyStroke;
    efi_event_t                 WaitForKey;
} simple_input_interface_t;

typedef efi_status_t (EFIAPI *efi_text_reset_t)(void *This, boolean_t ExtendedVerification);
typedef efi_status_t (EFIAPI *efi_text_output_string_t)(void *This, uint16_t *String);
typedef efi_status_t (EFIAPI *efi_text_test_string_t)(void *This, uint16_t *String);
typedef efi_status_t (EFIAPI *efi_text_query_mode_t)(void *This, uintn_t ModeNumber, uintn_t *Column, uintn_t *Row);
typedef efi_status_t (EFIAPI *efi_text_set_mode_t)(void *This, uintn_t ModeNumber);
typedef efi_status_t (EFIAPI *efi_text_set_attribute_t)(void *This, uintn_t Attribute);
typedef efi_status_t (EFIAPI *efi_text_clear_screen_t)(void *This);
typedef efi_status_t (EFIAPI *efi_text_set_cursor_t)(void *This, uintn_t Column, uintn_t Row);
typedef efi_status_t (EFIAPI *efi_text_enable_cursor_t)(void *This,  boolean_t Enable);

typedef struct {
    int32_t                     MaxMode;
    int32_t                     Mode;
    int32_t                     Attribute;
    int32_t                     CursorColumn;
    int32_t                     CursorRow;
    boolean_t                   CursorVisible;
} simple_text_output_mode_t;

typedef struct {
    efi_text_reset_t            Reset;
    efi_text_output_string_t    OutputString;
    efi_text_test_string_t      TestString;
    efi_text_query_mode_t       QueryMode;
    efi_text_set_mode_t         SetMode;
    efi_text_set_attribute_t    SetAttribute;
    efi_text_clear_screen_t     ClearScreen;
    efi_text_set_cursor_t       SetCursorPosition;
    efi_text_enable_cursor_t    EnableCursor;
    simple_text_output_mode_t   *Mode;
} simple_text_output_interface_t;

/* EFI services */

typedef struct {
  efi_physical_address_t Memory;
  uintn_t NoPages;
} memalloc_t;

typedef efi_status_t (EFIAPI *efi_allocate_pages_t)(efi_allocate_type_t Type, efi_memory_type_t MemoryType,
    uintn_t NoPages, efi_physical_address_t *Memory);
typedef efi_status_t (EFIAPI *efi_free_pages_t)(efi_physical_address_t Memory, uintn_t NoPages);
typedef efi_status_t (EFIAPI *efi_get_memory_map_t)(uintn_t *MemoryMapSize, efi_memory_descriptor_t *MemoryMap,
    uintn_t *MapKey, uintn_t *DescriptorSize, uint32_t *DescriptorVersion);
typedef efi_status_t (EFIAPI *efi_allocate_pool_t)(efi_memory_type_t PoolType, uintn_t Size, void **Buffer);
typedef efi_status_t (EFIAPI *efi_free_pool_t)(void *Buffer);
typedef efi_status_t (EFIAPI *efi_handle_protocol_t)(efi_handle_t Handle, guid_t *Protocol, void **Interface);
typedef efi_status_t (EFIAPI *efi_locate_handle_t)(efi_locate_search_type_t SearchType, guid_t *Protocol,
    void *SearchKey, uintn_t *BufferSize, efi_handle_t *Buffer);
typedef efi_status_t (EFIAPI *efi_exit_boot_services_t)(efi_handle_t ImageHandle, uintn_t MapKey);
typedef efi_status_t (EFIAPI *efi_stall_t)(uintn_t Microseconds);
typedef efi_status_t (EFIAPI *efi_locate_protocol_t)(guid_t *Protocol, void *Registration, void **Interface);

typedef struct {
    efi_table_header_t          Hdr;

    void*                       RaiseTPL;
    void*                       RestoreTPL;

    efi_allocate_pages_t        AllocatePages;
    efi_free_pages_t            FreePages;
    efi_get_memory_map_t        GetMemoryMap;
    efi_allocate_pool_t         AllocatePool;
    efi_free_pool_t             FreePool;

    void*                       CreateEvent;
    void*                       SetTimer;
    void*                       WaitForEvent;
    void*                       SignalEvent;
    void*                       CloseEvent;
    void*                       CheckEvent;

    void*                       InstallProtocolInterface;
    void*                       ReinstallProtocolInterface;
    void*                       UninstallProtocolInterface;
    efi_handle_protocol_t       HandleProtocol;
    efi_handle_protocol_t       PCHandleProtocol;
    void*                       RegisterProtocolNotify;
    efi_locate_handle_t         LocateHandle;
    void*                       LocateDevicePath;
    void*                       InstallConfigurationTable;

    void*                       LoadImage;
    void*                       StartImage;
    void*                       Exit;
    void*                       UnloadImage;
    efi_exit_boot_services_t    ExitBootServices;

    void*                       GetNextHighMonotonicCount;
    efi_stall_t                 Stall;
    void*                       SetWatchdogTimer;

    void*                       ConnectController;
    void*                       DisconnectController;

    void*                       OpenProtocol;
    void*                       CloseProtocol;
    void*                       OpenProtocolInformation;

    void*                       ProtocolsPerHandle;
    void*                       LocateHandleBuffer;
    efi_locate_protocol_t       LocateProtocol;
    void*                       InstallMultipleProtocolInterfaces;
    void*                       UninstallMultipleProtocolInterfaces;

    void*                       CalculateCrc32;
} efi_boot_services_t;

/* EFI system table */

#define ACPI_TABLE_GUID                 { 0xeb9d2d30, 0x2d88, 0x11d3, {0x9a, 0x16, 0x0, 0x90, 0x27, 0x3f, 0xc1, 0x4d} }
#define ACPI_20_TABLE_GUID              { 0x8868e871, 0xe4f1, 0x11d3, {0xbc, 0x22, 0x0, 0x80, 0xc7, 0x3c, 0x88, 0x81} }
#define SMBIOS_TABLE_GUID               { 0xeb9d2d31, 0x2d88, 0x11d3, {0x9a, 0x16, 0x0, 0x90, 0x27, 0x3f, 0xc1, 0x4d} }

typedef struct {
    guid_t      VendorGuid;
    void        *VendorTable;
} efi_configuration_table_t;

typedef struct {
    efi_table_header_t              Hdr;

    uint16_t                        *FirmwareVendor;
    uint32_t                        FirmwareRevision;

    efi_handle_t                    ConsoleInHandle;
    simple_input_interface_t        *ConIn;

    efi_handle_t                    ConsoleOutHandle;
    simple_text_output_interface_t  *ConOut;

    efi_handle_t                    ConsoleErrorHandle;
    simple_text_output_interface_t  *StdErr;

    void                            *RuntimeServices;
    efi_boot_services_t             *BootServices;

    uintn_t                         NumberOfTableEntries;
    efi_configuration_table_t       *ConfigurationTable;
} efi_system_table_t;

/* Device Path Protocol */

#define EFI_DEVICE_PATH_PROTOCOL_GUID  { 0x09576E91, 0x6D3F, 0x11D2, {0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B} }

typedef struct {
    uint8_t     Type;               /* 4 - Media Device Path */
    uint8_t     SubType;            /* 1 - Hard Disk */
    uint8_t     Length[2];
    uint32_t    PartitionNumber;
    uint64_t    PartitionStart;
    uint64_t    PartitionSize;
    guid_t      PartitionSignature; /* UniquePartitionGUID */
    uint8_t     PartitionFormat;    /* 2 - GPT */
    uint8_t     SignatureFormat;    /* 2 - GUID */
} __attribute__((packed)) efi_hard_disk_device_path_t;

/* Loaded image protocol */

#define EFI_LOADED_IMAGE_PROTOCOL_GUID { 0x5B1B31A1, 0x9562, 0x11d2, {0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B} }

typedef struct {
    uint32_t                Revision;
    efi_handle_t            ParentHandle;
    void                    *SystemTable;
    efi_handle_t            DeviceHandle;
    void                    *FilePath;
    void                    *Reserved;
    uint32_t                LoadOptionsSize;
    void                    *LoadOptions;
    void                    *ImageBase;
    uint64_t                ImageSize;
    efi_memory_type_t       ImageCodeType;
    efi_memory_type_t       ImageDataType;
} efi_loaded_image_protocol_t;

/* Simple File System Protocol */

#define EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID { 0x964e5b22, 0x6459, 0x11d2, {0x8e, 0x39, 0x0, 0xa0, 0xc9, 0x69, 0x72, 0x3b} }
#define EFI_FILE_INFO_GUID  { 0x9576e92, 0x6d3f, 0x11d2, {0x8e, 0x39, 0x0, 0xa0, 0xc9, 0x69, 0x72, 0x3b} }

#define EFI_FILE_MODE_READ      0x0000000000000001

typedef struct {
    uint64_t                Size;
    uint64_t                FileSize;
    uint64_t                PhysicalSize;
    efi_time_t              CreateTime;
    efi_time_t              LastAccessTime;
    efi_time_t              ModificationTime;
    uint64_t                Attribute;
    uint16_t                FileName[262];
} efi_file_info_t;

typedef struct efi_file_handle_s efi_file_handle_t;

typedef efi_status_t (EFIAPI *efi_volume_open_t)(void *This, efi_file_handle_t **Root);
typedef struct {
    uint64_t                Revision;
    efi_volume_open_t       OpenVolume;
} efi_simple_file_system_protocol_t;

typedef efi_status_t (EFIAPI *efi_file_open_t)(efi_file_handle_t *File, efi_file_handle_t **NewHandle, uint16_t *FileName,
    uint64_t OpenMode, uint64_t Attributes);
typedef efi_status_t (EFIAPI *efi_file_close_t)(efi_file_handle_t *File);
typedef efi_status_t (EFIAPI *efi_file_delete_t)(efi_file_handle_t *File);
typedef efi_status_t (EFIAPI *efi_file_read_t)(efi_file_handle_t *File, uintn_t *BufferSize, void *Buffer);
typedef efi_status_t (EFIAPI *efi_file_write_t)(efi_file_handle_t *File, uintn_t *BufferSize, void *Buffer);
typedef efi_status_t (EFIAPI *efi_file_get_pos_t)(efi_file_handle_t *File, uint64_t *Position);
typedef efi_status_t (EFIAPI *efi_file_set_pos_t)(efi_file_handle_t *File, uint64_t Position);
typedef efi_status_t (EFIAPI *efi_file_get_info_t)(efi_file_handle_t *File, guid_t *InformationType, uintn_t *BufferSize,
    void *Buffer);
typedef efi_status_t (EFIAPI *efi_file_set_info_t)(efi_file_handle_t *File, guid_t *InformationType, uintn_t BufferSize,
    void *Buffer);
typedef efi_status_t (EFIAPI *efi_file_flush_t)(efi_file_handle_t *File);

struct efi_file_handle_s {
    uint64_t                Revision;
    efi_file_open_t         Open;
    efi_file_close_t        Close;
    efi_file_delete_t       Delete;
    efi_file_read_t         Read;
    efi_file_write_t        Write;
    efi_file_get_pos_t      GetPosition;
    efi_file_set_pos_t      SetPosition;
    efi_file_get_info_t     GetInfo;
    efi_file_set_info_t     SetInfo;
    efi_file_flush_t        Flush;
};

/* Graphics Output Protocol */

#define EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID { 0x9042a9de, 0x23dc, 0x4a38, {0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a } }

typedef enum {
  PixelRedGreenBlueReserved8BitPerColor,
  PixelBlueGreenRedReserved8BitPerColor,
  PixelBitMask,
  PixelBltOnly,
  PixelFormatMax
} efi_gop_pixel_format_t;

typedef struct {
    uint32_t                RedMask;
    uint32_t                GreenMask;
    uint32_t                BlueMask;
    uint32_t                ReservedMask;
} efi_gop_pixel_bitmask_t;

typedef struct {
    uint32_t                Version;
    uint32_t                HorizontalResolution;
    uint32_t                VerticalResolution;
    efi_gop_pixel_format_t  PixelFormat;
    efi_gop_pixel_bitmask_t PixelInformation;
    uint32_t                PixelsPerScanLine;
} efi_gop_mode_info_t;

typedef struct {
    uint32_t                MaxMode;
    uint32_t                Mode;
    efi_gop_mode_info_t     *Information;
    uintn_t                 SizeOfInfo;
    efi_physical_address_t  FrameBufferBase;
    uintn_t                 FrameBufferSize;
} efi_gop_mode_t;

typedef efi_status_t (EFIAPI *efi_gop_query_mode_t)(void *This, uint32_t ModeNumber, uintn_t *SizeOfInfo,
    efi_gop_mode_info_t **Info);
typedef efi_status_t (EFIAPI *efi_gop_set_mode_t)(void *This, uint32_t ModeNumber);
typedef efi_status_t (EFIAPI *efi_gop_blt_t)(void *This, uint32_t *BltBuffer, uintn_t BltOperation,
    uintn_t SourceX, uintn_t SourceY, uintn_t DestinationX, uintn_t DestionationY, uintn_t Width, uintn_t Height, uintn_t Delta);

typedef struct {
    efi_gop_query_mode_t    QueryMode;
    efi_gop_set_mode_t      SetMode;
    efi_gop_blt_t           Blt;
    efi_gop_mode_t          *Mode;
} efi_gop_t;

/* EDID Protocol */

#define EFI_EDID_ACTIVE_GUID     { 0xbd8c1056, 0x9f36, 0x44ec, { 0x92, 0xa8, 0xa6, 0x33, 0x7f, 0x81, 0x79, 0x86 } }
#define EFI_EDID_DISCOVERED_GUID { 0x1c0c34f6, 0xd380, 0x41fa, { 0xa0, 0x49, 0x8a, 0xd0, 0x6c, 0x1a, 0x66, 0xaa } }

typedef struct {
  uint32_t SizeOfEdid;
  uint8_t *Edid;
} efi_edid_t;

/* GUID Partitioning Table */

#define EFI_PTAB_HEADER_ID  "EFI PART"
#define EFI_PART_TYPE_EFI_SYSTEM_PART_GUID  { 0xc12a7328, 0xf81f, 0x11d2, {0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b} }
/* this is actually the same as 1 << GRUB_GPT_PART_ATTR_OFFSET_LEGACY_BIOS_BOOTABLE */
#define EFI_PART_USED_BY_OS 0x0000000000000004

typedef struct {
  uint64_t  Signature;
  uint32_t  Revision;
  uint32_t  HeaderSize;
  uint32_t  CRC32;
  uint32_t  Reserved;
  uint64_t  MyLBA;
  uint64_t  AlternateLBA;
  uint64_t  FirstUsableLBA;
  uint64_t  LastUsableLBA;
  guid_t    DiskGUID;
  uint64_t  PartitionEntryLBA;
  uint32_t  NumberOfPartitionEntries;
  uint32_t  SizeOfPartitionEntry;
  uint32_t  PartitionEntryArrayCRC32;
} __attribute__((packed)) gpt_header_t;

typedef struct {
  guid_t    PartitionTypeGUID;
  guid_t    UniquePartitionGUID;
  uint64_t  StartingLBA;
  uint64_t  EndingLBA;
  uint64_t  Attributes;
  uint16_t  PartitionName[36];
} __attribute__((packed)) gpt_entry_t;

/* EFI System Partition */

typedef struct {
  uint8_t   jmp[3];
  char      oem[8];
  uint16_t  bps;
  uint8_t   spc;
  uint16_t  rsc;
  uint8_t   nf;
  uint8_t   nr0;
  uint8_t   nr1;
  uint16_t  ts16;
  uint8_t   media;
  uint16_t  spf16;
  uint16_t  spt;
  uint16_t  nh;
  uint32_t  hs;
  uint32_t  ts32;
  uint32_t  spf32;
  uint32_t  flg;
  uint32_t  rc;
  char      vol[6];
  char      fst[8];
  char      dmy[20];
  char      fst2[8];
} __attribute__((packed)) esp_bpb_t;

typedef struct {
  char      name[8];
  char      ext[3];
  uint8_t   attr[9];
  uint16_t  ch;
  uint32_t  attr2;
  uint16_t  cl;
  uint32_t  size;
} __attribute__((packed)) esp_dir_t;

/* PCI Option ROM */

typedef struct {
  uint8_t   magic[2];     /* 0xaa55 */
  uint16_t  InitializationSize;
  uint32_t  EfiSignature; /* 0x0EF1 */
  uint16_t  EfiSubsystem; /* 0xA */
  uint16_t  EfiMachineType;
  uint8_t   Reserved[0x0A];
  uint16_t  EfiImageHeaderOffset;
  uint32_t  PcirOffset;
  uint32_t  Signature;    /* "PCIR" */
  uint16_t  VendorId;
  uint16_t  DeviceId;
  uint16_t  Reserved0;
  uint16_t  Length;
  uint8_t   Revision;
  uint8_t   ClassCode[3];
  uint16_t  ImageLength;
  uint16_t  CodeRevision;
  uint8_t   CodeType;
  uint8_t   Indicator;
  uint16_t  Reserved1;
  uint8_t   checksum;
  uint8_t   Reserved2;
} __attribute__((packed)) pcirom_t;

/*** FOSSBIOS ***/

/* Boot Catalog */

typedef struct {
  uint8_t   magic[6];   /* 0xB0, 0x07, 0xCA, 0x7A, 0x10, 0xC0 */
  uint8_t   chksum;     /* same as in PCI ROM, ACPI etc. sum to zero */
  uint8_t   numentries;
  struct {
    uint16_t  arch;     /* 0 = x86, 1 = ARM, 2 = RISC-V */
    uint8_t   wordsize; /* in bytes */
    uint8_t   endian;   /* 0 = little-endian */
    uint32_t  lba;
  } __attribute__((packed)) entries[256];
} __attribute__((packed)) fb_boot_t;

/* System Services */

#define FB_MEMENT_TYPE(size) ((size) & 7)
#define FB_MEM_FREE     0
#define FB_MEM_USED     1 /* allocated for Boot Loader / OS, reclaimable */
#define FB_MEM_BIOS     2 /* allocated for FOSSBIOS internally, reclaimable */
#define FB_MEM_ROM      3 /* read-only ROM area */
#define FB_MEM_MMIO     4 /* memory mapped input / ouput */
#define FB_MEM_NVS      5 /* non-volatile data, saved during hybernation */
#define FB_MEM_BAD      7 /* other unusable */

typedef struct {
  uint64_t  base;
  uint64_t  size;
} __attribute__((packed)) fb_mement_t;

typedef int (*fb_udelay_t)(uint64_t usec);
typedef uint8_t* (*fb_exit_t)(void);

typedef struct {
  uint8_t       magic[4];     /* 'S','Y','S',16 */
  uint32_t      nummap;
  fb_mement_t   *memmap;
  void          *boot;
  void          *shell;
  fb_exit_t     exit;
  fb_udelay_t   udelay;
  void          *memcpy;
  void          *memset;
  void          *memcmp;
  void          *alloc;
  void          *free;
  void          *mark;
  void          *getenv;
  void          *setenv;
  void          *printf;
  void          *devadd;
  void          *resadd;
  void          *memadd;
  void          *lstadd;
} __attribute__((packed)) fb_system_t;

/* Video Services */

typedef struct {
  uint32_t  width;
  uint32_t  height;
  uint32_t  pitch;
  uint8_t   bpp;
  uint8_t   red;
  uint8_t   green;
  uint8_t   blue;
  uint8_t   reserved[16];
} __attribute__((packed)) fb_vidmode_t;

typedef int (*fb_vsetmode_t)(uint16_t mode);

typedef struct {
  uint8_t       magic[4];     /* 'V','I','D',6 */
  uint16_t      nummodes;
  uint16_t      curmode;
  void          *next;
  fb_vidmode_t  *vidmodes;
  uint8_t       *edid;
  uint8_t       *lfb;
  fb_vsetmode_t setmode;
  void          *blit;
} __attribute__((packed)) fb_video_t;

/* Storage Service */

typedef int (*fb_sread_t)(uint16_t dev, uint32_t blksize, uint64_t lba, void *buf);

typedef struct {
  uint8_t       magic[4];     /* 'B','L','K',3 */
  uint16_t      num;
  uint16_t      size;
  void          *data;
  fb_sread_t    read;
  void          *write;
} __attribute__((packed)) fb_storage_t;

/* Serial Services */

typedef void (*fb_ssend_t)(uint16_t dev, uint8_t chr);
typedef uint8_t* (*fb_srecv_t)(uint16_t dev);
typedef int (*fb_spend_t)(uint16_t dev);
typedef void (*fb_ssetmode_t)(uint16_t dev, uint32_t baud, uint8_t bits, uint8_t parity, uint8_t stop);

typedef struct {
  uint8_t       magic[4];     /* 'S','E','R',5 */
  uint16_t      num;
  uint16_t      size;
  void          *data;
  fb_ssend_t    send;
  fb_srecv_t    recv;
  fb_spend_t    pend;
  fb_ssetmode_t setmode;
} __attribute__((packed)) fb_serial_t;

/* Input Services */

typedef uint32_t (*fb_getkey_t)(void);
typedef uint32_t (*fb_haskey_t)(void);

typedef struct {
  uint8_t       magic[4];     /* 'I','N','P',4 */
  uint32_t      mbz;
  fb_getkey_t   getkey;
  fb_haskey_t   haskey;
  void          *getptr;
  void          *setptr;
} __attribute__((packed)) fb_input_t;

/* FOSSBIOS Main Structure */

typedef struct {
  uint8_t       magic[4];     /* 'F','B','S',12 */
  uint16_t      arch;
  uint8_t       wordsize;
  uint8_t       endian;
  void          *oem;
  void          *conf;
  fb_system_t   *system;
  void          *proc;
  fb_video_t    *video;
  fb_storage_t  *storage;
  fb_serial_t   *serial;
  fb_input_t    *input;
  void          *clock;
  void          *audio;
  void          *net;
  void          *power;
} __attribute__((packed)) fossbios_t;

#define FOSSBIOS_MAGIC  0xF055B105
typedef void (*fb_loader_t)(uint32_t magic, fossbios_t *mainstruct, uint16_t bootdev);

/* FOSSBIOS ROM module */

typedef struct {
  uint8_t   magic[2];     /* 0xaa55 */
  uint16_t  size;
  uint8_t   fossmagic[4]; /* 0xF0, 0x55, 0xB1, 0x05 */
  uint16_t  arch;
  uint8_t   wordsize;
  uint8_t   endian;
  uint32_t  compressed;
  uint32_t  uncompressed;
  uint8_t   type;
  uint8_t   initorder;
  uint16_t  numreloc;
  uint32_t  pcir_offs;
  uint8_t   pcir[24];
  uint32_t  entry;
  uint32_t  bss;
  uint32_t  crc;
} __attribute__((packed)) fb_rom_t;

/*** ACPI ***/

typedef struct {
    char magic[8];      /* "RSD PTR " */
    uint8_t chksum;
    char OEM[6];
    uint8_t rev;        /* 2 */
    uint32_t rsdt;
} __attribute__((packed)) rsdp_t;

typedef struct {
    char magic[4];
    uint32_t size;
    uint8_t rev;
    uint8_t chksum;
    char OEM[6];
    char OEMtableid[8];
    uint32_t OEMrev;
    uint32_t creatid;
    uint32_t creatrev;
} __attribute__((packed)) sdt_hdr_t;

typedef struct {
    sdt_hdr_t hdr;
    uint32_t table_ptr[2];
} __attribute__((packed)) rsdt_t;

typedef struct {
    uint8_t type;   /* 0 processor */
    uint8_t size;   /* 8 */
    uint8_t acpi_id;
    uint8_t apic_id;
    uint32_t flags; /* bit 0: enabled, bit 1: available */
} __attribute__((packed)) cpu_entry_t;

typedef struct {
    sdt_hdr_t hdr;  /* magic "APIC" */
    uint32_t lapic_addr;
    uint32_t flags;
    cpu_entry_t cpus[4];
} __attribute__((packed)) apic_t;

typedef struct {
    sdt_hdr_t hdr;  /* magic "FACP" */
    uint32_t firmwarectrl;
    uint32_t dsdt;
    uint8_t  reserved[96];
    uint64_t x_dsdt;
} __attribute__((packed)) fadt_t;
