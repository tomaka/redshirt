/*
 * example/kernel.c
 * https://gitlab.com/bztsrc/simpleboot
 *
 * Copyright (C) 2023 bzt (bztsrc@gitlab), MIT license
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
 * @brief An example Multiboot2 compliant kernel for the Simpleboot loader
 * https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html
 *
 * This is a very minimal "kernel" that just dumps the MBI to the serial console.
 * The main function is 99.9% identical to the one in the Multiboot2 spec (that's
 * why the identation is so ugly).
 */

#include <simpleboot.h>

void printf(char *fmt, ...);
void dumpacpi(uint64_t addr);
void dumpuuid(uint8_t *uuid);

/*****************************************
 *          kernel entry point           *
 *****************************************/
void _start(uint32_t magic, uintptr_t addr)
{
    multiboot_tag_t *tag, *last;
    multiboot_mmap_entry_t *mmap;
    multiboot_tag_framebuffer_t *tagfb;
    unsigned int size;

    /* if everything else fails, this always works */
/*
    __asm__ __volatile__("":"=a"(magic),"=b"(addr)::);
*/

    /* since this might run on multiple cores, do some locking to avoid messing up each other's output */
    while(*((volatile uint8_t*)0x558)) {}; *((volatile uint8_t*)0x558) = 1;

    /*  Am I booted by a Multiboot-compliant boot loader? */
    if (magic != MULTIBOOT2_BOOTLOADER_MAGIC) {
      printf ("Invalid magic number: 0x%x\n", (unsigned) magic);
      goto halt;
    }

    if (addr & 7) {
      printf ("Unaligned MBI: 0x%x\n", addr);
      goto halt;
    }

    /*  Dump the MBI tags that we've received */
    size = ((multiboot_info_t*)addr)->total_size;
    printf ("\nAnnounced MBI size 0x%x\n", size);
    for (tag = (multiboot_tag_t *) (addr + 8), last = (multiboot_tag_t *) (addr + size);
         tag < last && tag->type != MULTIBOOT_TAG_TYPE_END;
         tag = (multiboot_tag_t *) ((uint8_t *) tag + ((tag->size + 7) & ~7)))
    {
      printf ("Tag 0x%x, Size 0x%x\n", tag->type, tag->size);
      switch (tag->type) {
        case MULTIBOOT_TAG_TYPE_CMDLINE:
          printf ("Command line = %s\n",
                  ((multiboot_tag_cmdline_t *) tag)->string);
          break;
        case MULTIBOOT_TAG_TYPE_BOOT_LOADER_NAME:
          printf ("Boot loader name = %s\n",
                  ((multiboot_tag_loader_t *) tag)->string);
          break;
        case MULTIBOOT_TAG_TYPE_MODULE:
          printf ("Module at 0x%x-0x%x. Command line %s\n",
                  ((multiboot_tag_module_t *) tag)->mod_start,
                  ((multiboot_tag_module_t *) tag)->mod_end,
                  ((multiboot_tag_module_t *) tag)->string);
          break;
        case MULTIBOOT_TAG_TYPE_MMAP:
          {
            printf ("mmap\n");
            for (mmap = ((multiboot_tag_mmap_t *) tag)->entries;
                 (uint8_t *) mmap < (uint8_t *) tag + tag->size;
                 mmap = (multiboot_mmap_entry_t *) ((uintptr_t) mmap
                    + ((multiboot_tag_mmap_t *) tag)->entry_size))
              printf (" base_addr = 0x%8x%8x,"
                      " length = 0x%8x%8x, type = 0x%x %s, res = 0x%x\n",
                      (unsigned) (mmap->base_addr >> 32),
                      (unsigned) (mmap->base_addr & 0xffffffff),
                      (unsigned) (mmap->length >> 32),
                      (unsigned) (mmap->length & 0xffffffff),
                      (unsigned) mmap->type,
                      mmap->type == MULTIBOOT_MEMORY_AVAILABLE ? "free" : (
                      mmap->type == MULTIBOOT_MEMORY_ACPI_RECLAIMABLE ? "ACPI" : (
                      mmap->type == MULTIBOOT_MEMORY_NVS ? "ACPI NVS" : "used")),
                      (unsigned) mmap->reserved);
          }
          break;
        case MULTIBOOT_TAG_TYPE_FRAMEBUFFER:
          {
            tagfb = (multiboot_tag_framebuffer_t *) tag;
            printf ("framebuffer\n");
            printf (" address 0x%8x%8x pitch %d\n",
                (unsigned) (tagfb->framebuffer_addr >> 32),
                (unsigned) (tagfb->framebuffer_addr & 0xffffffff),
                tagfb->framebuffer_pitch);
            printf (" width %d height %d depth %d bpp\n",
                tagfb->framebuffer_width,
                tagfb->framebuffer_height,
                tagfb->framebuffer_bpp);
            printf (" red channel:   at %d, %d bits\n",
                tagfb->framebuffer_red_field_position,
                tagfb->framebuffer_red_mask_size);
            printf (" green channel: at %d, %d bits\n",
                tagfb->framebuffer_green_field_position,
                tagfb->framebuffer_green_mask_size);
            printf (" blue channel:  at %d, %d bits\n",
                tagfb->framebuffer_blue_field_position,
                tagfb->framebuffer_blue_mask_size);
            break;
          }
        case MULTIBOOT_TAG_TYPE_EFI64:
          printf ("EFI system table 0x%x\n",
                  ((multiboot_tag_efi64_t *) tag)->pointer);
          break;
        case MULTIBOOT_TAG_TYPE_EFI64_IH:
          printf ("EFI image handle 0x%x\n",
                  ((multiboot_tag_efi64_t *) tag)->pointer);
          break;
        case MULTIBOOT_TAG_TYPE_SMBIOS:
          printf ("SMBIOS table major %d minor %d\n",
                  ((multiboot_tag_smbios_t *) tag)->major,
                  ((multiboot_tag_smbios_t *) tag)->minor);
          break;
        case MULTIBOOT_TAG_TYPE_ACPI_OLD:
          printf ("ACPI table (1.0, old RSDP)");
          dumpacpi ((uint64_t)*((uint32_t*)&((multiboot_tag_old_acpi_t *) tag)->rsdp[16]));
          break;
        case MULTIBOOT_TAG_TYPE_ACPI_NEW:
          printf ("ACPI table (2.0, new RSDP)");
          dumpacpi (*((uint64_t*)&((multiboot_tag_new_acpi_t *) tag)->rsdp[24]));
          break;
        /* additional, not in the original Multiboot2 spec */
        case MULTIBOOT_TAG_TYPE_EDID:
          printf ("EDID info\n");
          printf (" manufacturer ID %02x%02x\n",
            ((multiboot_tag_edid_t *) tag)->edid[8], ((multiboot_tag_edid_t *) tag)->edid[9]);
          printf (" EDID ID %02x%02x Version %d Rev %d\n",
            ((multiboot_tag_edid_t *) tag)->edid[10], ((multiboot_tag_edid_t *) tag)->edid[11],
            ((multiboot_tag_edid_t *) tag)->edid[18], ((multiboot_tag_edid_t *) tag)->edid[19]);
          printf (" monitor type %02x size %d cm x %d cm\n",
            ((multiboot_tag_edid_t *) tag)->edid[20], ((multiboot_tag_edid_t *) tag)->edid[21],
            ((multiboot_tag_edid_t *) tag)->edid[22]);
          break;
        case MULTIBOOT_TAG_TYPE_SMP:
          printf ("SMP supported\n");
          printf (" %d core(s)\n", ((multiboot_tag_smp_t*) tag)->numcores);
          printf (" %d running\n", ((multiboot_tag_smp_t*) tag)->running);
          printf (" %02x bsp id\n", ((multiboot_tag_smp_t*) tag)->bspid);
          break;
        case MULTIBOOT_TAG_TYPE_PARTUUID:
          printf ("Partition UUIDs\n");
          printf (" boot "); dumpuuid(((multiboot_tag_partuuid_t*) tag)->bootuuid);
          if(tag->size >= 40) {
            printf (" root "); dumpuuid(((multiboot_tag_partuuid_t*) tag)->rootuuid);
          }
          break;
        default:
          printf ("---unknown MBI tag, this shouldn't happen with Simpleboot/Easyboot!---\n");
          goto halt;
        }
    }
    tag = (multiboot_tag_t *) ((uint8_t *) tag + ((tag->size + 7) & ~7));
    printf ("Total MBI size 0x%x %s\n", (uintptr_t)tag - addr, ((uintptr_t)tag - addr) == size ? "OK" : "ERR");

    /* there's nowhere to return to, halt machine */
halt:
    *((volatile uint8_t*)0x558) = 0;
#ifdef __aarch64__
    __asm__ __volatile__("1: wfe; b 1b");
#else
    __asm__ __volatile__("1: cli; hlt; jmp 1b");
#endif
}

/**
 * Display (extremely minimal) formated message on serial
 */
void printf(char *fmt, ...)
{
    __builtin_va_list args;
    int arg, len, sign, i;
    unsigned int uarg;
    char *p, tmpstr[19], n;
    /* macro to put a character on serial console */
#ifdef __aarch64__
#define mmio_base   0x3F000000
#define UART0_DR    ((volatile uint32_t*)(mmio_base+0x00201000))
#define UART0_FR    ((volatile uint32_t*)(mmio_base+0x00201018))
#define PUTC(c)     do{do{ __asm__ __volatile__("nop");} while(*UART0_FR&0x20); *UART0_DR=c;}while(0)
#else
#define PUTC(c)     __asm__ __volatile__( \
                    "xorl %%ebx, %%ebx; movb %0, %%bl;" \
                    "movl $10000,%%ecx;" \
                    "1:inb %%dx, %%al;pause;" \
                    "cmpb $0xff,%%al;je 2f;" \
                    "dec %%ecx;jz 2f;" \
                    "andb $0x20,%%al;jz 1b;" \
                    "subb $5,%%dl;movb %%bl, %%al;outb %%al, %%dx;2:" \
                    ::"a"(c),"d"(0x3fd): "rbx", "rcx");
#endif
    /* parse format and print */
    __builtin_va_start(args, fmt);
    arg = 0;
    while(*fmt) {
        if(*fmt == '%') {
            fmt++;
            if(*fmt == '%') goto put;
            len=0; while(*fmt >= '0' && *fmt <= '9') { len *= 10; len += *fmt - '0'; fmt++; }
            if(*fmt == 'c') { arg = __builtin_va_arg(args, int); PUTC((uint8_t)arg); fmt++; continue; } else
            if(*fmt == 'd') {
                arg = __builtin_va_arg(args, int);
                sign = 0; if((int)arg < 0) { arg = -arg; sign++; }
                i = 18; tmpstr[i] = 0;
                do { tmpstr[--i] = '0' + (arg % 10); arg /= 10; } while(arg != 0 && i > 0);
                if(sign) tmpstr[--i] = '-';
                if(len > 0 && len < 18) { while(i > 18 - len) tmpstr[--i] = ' '; }
                p = &tmpstr[i];
                goto putstring;
            } else
            if(*fmt == 'x') {
                uarg = __builtin_va_arg(args, unsigned int);
                i = 16; tmpstr[i] = 0;
                do { n = uarg & 0xf; tmpstr[--i] = n + (n > 9 ? 0x37 : 0x30); uarg >>= 4; } while(uarg != 0 && i > 0);
                if(len > 0 && len <= 16) { while(i > 16 - len) tmpstr[--i] = '0'; }
                p = &tmpstr[i];
                goto putstring;
            } else
            if(*fmt == 's') {
                p = __builtin_va_arg(args, char*);
putstring:      if(p == (void*)0) p = "(null)";
                while(*p) PUTC(*p++);
            }
        } else {
put:        PUTC(*fmt);
        }
        fmt++;
    }
    __builtin_va_end(args);
}

/**
 * Print a binary UUID in human readable form
 */
void dumpuuid(uint8_t *uuid)
{
    printf("%02x%02x%02x%02x-%02x%02x-%02x%02x-%02x%02x%02x%02x%02x%02x%02x%02x\n",
        uuid[3], uuid[2], uuid[1], uuid[0],
        uuid[5], uuid[4],
        uuid[7], uuid[6],
        uuid[8], uuid[9], uuid[10], uuid[11], uuid[12], uuid[13], uuid[14], uuid[15]);
}

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
    char magic[4];
    uint32_t size;
    uint8_t rev;
    uint8_t chksum;
    uint8_t res0[30];
    uint32_t dsdt;
    uint8_t  reserved[96];
    uint64_t x_dsdt;
} __attribute__((packed)) fadt_t;

/**
 * Dump ACPI tables
 */
void dumpacpi(uint64_t addr)
{
    uint8_t *ptr, *end, *p;
    sdt_hdr_t *hdr = (sdt_hdr_t*)addr, *tbl = 0;

    /* print root table, either RSDT or XSDT */
    printf(" 0x%08x%08x %c%c%c%c size %d\n",
        addr >> 32, addr & 0xffffffff,
        hdr->magic[0], hdr->magic[1], hdr->magic[2], hdr->magic[3],
        hdr->size);
    /* iterate on tables */
    if(hdr->magic[1] == 'S' && hdr->magic[2] == 'D' && hdr->magic[3] == 'T')
        for(ptr = (uint8_t*)(addr + sizeof(sdt_hdr_t)), end = (uint8_t*)(addr + hdr->size);
          ptr < end; ptr += hdr->magic[0] == 'X' ? 8 : 4) {
            /* with RSDT we have 32-bit addresses, but with XSDT 64-bit */
            tbl = (hdr->magic[0] == 'X' ?
                (sdt_hdr_t*)((uintptr_t)*((uint64_t*)ptr)) :
                (sdt_hdr_t*)((uintptr_t)*((uint32_t*)ptr)));
            printf("  0x%08x%08x %c%c%c%c size %d",
                (uint64_t)tbl >> 32, (uint64_t)tbl & 0xffffffff,
                tbl->magic[0], tbl->magic[1], tbl->magic[2], tbl->magic[3],
                tbl->size);
            /* if it's FADT, print the DSDT in it too. There's a 32-bit address and a 64-bit address for it as well */
            if(tbl->magic[0] == 'F' && tbl->magic[1] == 'A' && tbl->magic[2] == 'C' && tbl->magic[3] == 'P') {
                p = tbl->rev >= 2 && tbl->size > 148 ? (uint8_t*)(uintptr_t)((fadt_t*)tbl)->x_dsdt :
                    (uint8_t*)(uintptr_t)((fadt_t*)tbl)->dsdt;
                /* it is possible that the DSDT data is actually GUDT or DTB encoded (loader's feature, not in ACPI) */
                if(p[0] == 0xD0 && p[1] == 0x0D && p[2] == 0xFE && p[3] == 0xED)
                    printf(" (DTB ");
                else
                    printf(" (%c%c%c%c ", p[0], p[1], p[2], p[3]);
                /* print out address */
                if(tbl->rev >= 2 && tbl->size > 148)
                    printf("0x%08x%08x)", ((fadt_t*)tbl)->x_dsdt >> 32, ((fadt_t*)tbl)->x_dsdt & 0xffffffff);
                else
                    printf("0x%08x)", p);
            }
            printf("\n");
        }
}
