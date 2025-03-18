/*
 * src/loader_cb.c
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
 * @brief The main Simpleboot loader program as a coreboot payload
 *
 * Memory layout when booted with coreboot:
 *      0x510 -   0x5A0   GDT and AP startup data
 *     0x1000 - 0x20000   paging tables (0x8000 - 0x80FF relocated AP startup code)
 *    0x20000 - 0x90000   config + logo + tags; from the top to bottom: kernel's stack
 *    0x90000 - 0xA0000   Linux kernel only: zero page + cmdline
 *    0xA0000 - 0xFFFFF   VRAM and BIOS ROM
 *   0x100000 -      x    kernel segments, followed by the modules, each page aligned (from 1M)
 * 0x03000000 -      x    libpayload and loader (from 48M)
 *
 * Compilation:
 *
 * Step 1: Install dependencies
 * ----------------------------
 *
 * Install coreboot's dependencies: bison, build-essentials, curl, flex, git, gnat, libncurses5-dev, m4, zlib.
 * Please refer to https://doc.coreboot.org/tutorial/part1.html to an up-to-date list.
 *
 * Step 2: Get coreboot
 * --------------------
 *
 * git clone https://review.coreboot.org/coreboot && cd coreboot && git submodule update --init
 *
 * Step 3: Add Simpleboot to coreboot
 * ----------------------------------
 *
 * Replace "/path/to/simpleboot" with the directory where you've downloaded Simpleboot.
 * NOTE: a symlink really should work here, but it DOES NOT. You must physically copy the directory.
 *
 * cp -r /path/to/simpleboot payloads/external
 *
 * If you haven't downloaded Simpleboot yet, then this also works:
 *
 * git clone https://gitlab.com/bztsrc/simpleboot payloads/external
 *
 * Step 4: Create coreboot toolchain
 * ---------------------------------
 *
 * This will take a while. Replace "CPUS=4" with the number of CPU cores you have. This step takes a while.
 *
 * make crossgcc-i386 CPUS=4
 *
 * Step 5: Configure coreboot
 * --------------------------
 *
 * Now configure coreboot for your motherboard (or qemu) and Simpleboot.
 *
 * make menuconfig
 *     select 'Mainboard' menu
 *     beside 'Mainboard vendor' should be '(Emulation)'
 *     beside 'Mainboard model' should be 'QEMU x86 i440fx/piix4'
 *     select 'Exit'
 *     select 'Devices'
 *     select 'Display'
 *     beside 'Framebuffer mode' should be 'Linear "high-resolution" framebuffer'
 *     select 'Exit'
 *     select 'Exit'
 *     select 'Payload' menu
 *     select 'Payload to add'
 *     choose 'Simpleboot'
 *     select 'Exit'
 *     select 'Exit'
 * Also add the desired device drivers (like AHCI and maybe USB if you want to boot from stick too)
 * and set the desired screen resolution. Finally
 *     select 'Exit'
 *     select 'Yes'
 *
 * Step 5: Build coreboot
 * ----------------------
 *
 * make -C payloads/external/simpleboot && make
 *
 * Step 6: Add files to ROM (optional)
 * -----------------------------------
 *
 * If you wish, you can add your custom simpleboot.cfg to the ROM. If you don't, then it will be loaded from disk.
 *
 * ./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/simpleboot.cfg -n simpleboot.cfg
 *
 * If you wish, you can also add your custom logo.tga (it will be displayed automatically if added).
 *
 * ./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/logo.tga -n logo.tga
 *
 * If you wish, you can add a device tree blob too (in GUDT, FDT and DSDT; depending which format your kernel understands).
 *
 * ./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/devices.gud -n devices
 * ./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/devices.dtb -n devices
 * ./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/DSDT.aml -n devices
 *
 * Finally, you can also add a default kernel and default initrd too (not just set their default filenames
 * with -k and -i, but their actual file content can be embedded in the ROM):
 *
 * ./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/kernel -n kernel
 * ./build/cbfstool build/coreboot.rom add -t raw -f /path/to/your/initrd -n initrd
 *
 * If exists, these are only used as a very last resort fallback option when absolutely all the other methods have failed.
 *
 * Step 7: Test your new ROM
 * -------------------------
 *
 * Replace "/path/to/your/disk.img" with your disk image.
 *
 * qemu-system-x86_64 -bios build/coreboot.rom -drive file=/path/to/your/disk.img,format=raw -serial stdio
 *
 */

/**
 * The longest path we can handle
 */
#define PATH_MAX 1024
/**
 * Maximum size of MBI tags in pages (30 * 4096 = 122k)
 */
#define TAGS_MAX 30

/*
 * DO NOT try to use payloads/libpayload/Makefile.payload that would compile all C files, not just this one
 * DO NOT try to include payload.h that's a real header dependency hell! Just copy'n'paste the prototypes that we actually need
 */
#define CBFS_METADATA_MAX_SIZE 256
#include <kconfig.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <sysinfo.h>
#include <storage/storage.h>
#if CONFIG(LP_ARCH_X86)
#include <x86/arch/io.h>
#endif
#if CONFIG(LP_USB)
static void* usbdev = NULL;
void usbdisk_create(void* dev) { if(!usbdev) usbdev = dev; }
void usbdisk_remove(void* dev) { if(usbdev == dev) usbdev = NULL; }
#endif
void arch_ndelay(uint64_t n);
void console_init(void);
int havekey(void);
int getchar(void);
void video_console_clear(void);
void storage_initialize(void);
int usb_initialize(void);
int usb_exit(void);
void usb_poll(void);
int readwrite_blocks_512(void *dev, int start, int n, int dir, uint8_t *buf);
void halt(void);
ssize_t _cbfs_boot_lookup(const char *name, int force_ro, void *mdata);
void *_cbfs_load(const char *name, void *buf, size_t *size_inout, int force_ro);
extern char _end;
static inline size_t cbfs_load(const char *name, void *buf, size_t size)
{ return _cbfs_load(name, buf, &size, 0) ? size : 0; }
static inline size_t cbfs_get_size(const char *name)
{
    uint8_t mdata[CBFS_METADATA_MAX_SIZE];
    size_t ret;
    ret = _cbfs_boot_lookup(name, 0, &mdata) < 0 ? 0 : (mdata[8] << 24) | (mdata[9] << 16) | (mdata[10] << 8) | mdata[11];
    return ret;
}

#include "../simpleboot.h"
#include "loader.h"
#include "inflate.h"

#define sleep(n) arch_ndelay((uint64_t)(n) * 1000000)

#define send_ipi(a,m,v) do { \
        while(*((volatile uint32_t*)(lapic + 0x300)) & (1 << 12)) __asm__ __volatile__ ("pause" : : : "memory"); \
        *((volatile uint32_t*)(lapic + 0x310)) = (*((volatile uint32_t*)(lapic + 0x310)) & 0x00ffffff) | (a << 24); \
        *((volatile uint32_t*)(lapic + 0x300)) = (*((volatile uint32_t*)(lapic + 0x300)) & m) | v;  \
    } while(0)

esp_bpb_t *bpb;
multiboot_mmap_entry_t *memmap;
multiboot_tag_module_t *initrd;
multiboot_tag_framebuffer_t vidmode;
uint64_t mmio_base, file_size, rsdp_ptr, dsdt_ptr, file_buf, page_buf, ram, *pt, root_dir;
uint32_t fb_w, fb_h, fb_bpp, fb_bg, logo_size, verbose, num_memmap, pb_b, pb_m, pb_l, rq, bkp, smp;
uint16_t wcname[PATH_MAX];
uint8_t rpi, *tags_buf, *tags_ptr, *logo_buf, *kernel_entry, kernel_mode, kernel_buf[4096], *kernel_mem, *pb_fb;
char *conf_buf, *kernel, *cmdline;
linux_boot_params_t *zero_page;

/**
 * Convert hex string into integer
 */
char *gethex(char *ptr, uint32_t *ret)
{
    int len = 2;
    *ret = 0;
    for(;len--;ptr++) {
        if(*ptr>='0' && *ptr<='9') {          *ret <<= 4; *ret += (uint32_t)(*ptr - '0'); }
        else if(*ptr >= 'a' && *ptr <= 'f') { *ret <<= 4; *ret += (uint32_t)(*ptr - 'a' + 10); }
        else if(*ptr >= 'A' && *ptr <= 'F') { *ret <<= 4; *ret += (uint32_t)(*ptr - 'A' + 10); }
        else break;
    }
    return ptr;
}

/**
 * Convert decimal string to integer
 */
char *getint(char *ptr, uint32_t *ret)
{
    for(*ret = 0; *ptr >= '0' && *ptr <= '9'; ptr++) { *ret *= 10; *ret += (uint32_t)(*ptr - '0'); }
    return ptr;
}

/**
 * Hexdump
 */
void hexdump(void *data, int n)
{
    uint8_t *ptr = (uint8_t*)data;
    int i, j;
    for(j = 0; j < n; j++, ptr += 16) {
        printf("%08x: ", (uint32_t)(uintptr_t)ptr);
        for(i = 0; i < 16; i++) printf("%02x ", ptr[i]);
        printf(" ");
        for(i = 0; i < 16; i++) printf("%c", ptr[i] >= 32 && ptr[i] < 127 ? ptr[i] : '.');
        printf("\r\n");
    }
}

/**************** Progress bar ****************/

/**
 * Initialize the progress bar
 */
uint64_t pb_init(uint64_t size)
{
    uint32_t c, i, x;

    pb_fb = NULL; pb_m = (size >> 9) + 1; pb_l = 0;
    if(!vidmode.framebuffer_addr || !size || pb_m < vidmode.framebuffer_width - 4) return 0;
    pb_b = (vidmode.framebuffer_bpp + 7) >> 3;
    pb_fb = (uint8_t*)(uintptr_t)vidmode.framebuffer_addr + (vidmode.framebuffer_height - 4) * vidmode.framebuffer_pitch + 2 * pb_b;
    c = FB_COLOR(32, 32, 32);
    for(i = x = 0; x < vidmode.framebuffer_width - 4; x++, i += pb_b)
        switch(vidmode.framebuffer_bpp) {
            case 15: case 16: *((uint16_t*)(pb_fb + i)) = *((uint16_t*)(pb_fb + vidmode.framebuffer_pitch + i)) = c; break;
            case 24: case 32: *((uint32_t*)(pb_fb + i)) = *((uint32_t*)(pb_fb + vidmode.framebuffer_pitch + i)) = c; break;
        }
    return size / (vidmode.framebuffer_width - 4);
}

/**
 * Draw the progress bar
 */
void pb_draw(uint64_t curr)
{
    uint32_t i;

    if(pb_fb) {
        i = pb_l; pb_l = ((curr >> 9) * (vidmode.framebuffer_width - 4) / pb_m) * pb_b;
        for(; i < pb_l; i++)
            switch(vidmode.framebuffer_bpp) {
                case 15: case 16: *((uint16_t*)(pb_fb + i)) = *((uint16_t*)(pb_fb + vidmode.framebuffer_pitch + i)) = 0xFFFF; break;
                case 24: case 32: *((uint32_t*)(pb_fb + i)) = *((uint32_t*)(pb_fb + vidmode.framebuffer_pitch + i)) = 0xFFFFFF; break;
            }
    }
}

/**
 * Close the progress bar
 */
void pb_fini(void)
{
    uint32_t i, x;

    if(pb_fb)
        for(i = x = 0; x < vidmode.framebuffer_width - 2; x++, i += pb_b)
            switch(vidmode.framebuffer_bpp) {
                case 15: case 16: *((uint16_t*)(pb_fb + i)) = *((uint16_t*)(pb_fb + vidmode.framebuffer_pitch + i)) = fb_bg; break;
                case 24: case 32: *((uint32_t*)(pb_fb + i)) = *((uint32_t*)(pb_fb + vidmode.framebuffer_pitch + i)) = fb_bg; break;
            }
    pb_fb = NULL;
}

uint64_t data_lba, fat_lba;
uint8_t vbr[512], data[512];
uint32_t fat[1024], fat_cache, file_clu;
uint16_t lfn[261];
guid_t bootuuid;

/**
 * Load a sector from boot drive using libpayload
 */
void loadsec(uint64_t lba, void *dst)
{
    uint8_t i;

    if(havekey()) { getchar(); rq = 1; }
#if CONFIG(LP_USB)
    /* USB storage */
    if(usbdev)
        readwrite_blocks_512(usbdev, lba, 1, 0x80/*cbw_direction_data_in*/, dst);
    else
#endif
    /* AHCI storage */
    if(storage_device_count())
        storage_read_blocks512(0, lba, 1, dst);
#if CONFIG(LP_ARCH_X86)
    else {
        /* fallback primary ATA IDE */
        i = inb(0x1F6);
        if(i != 0 && i != 0xFF) {
            while((inb(0x1F7) & 0xC0) != 0x40);
            outb(1, 0x1F2);
            outb(lba, 0x1F3);
            outb(lba >> 8, 0x1F4);
            outb(lba >> 16, 0x1F5);
            outb((lba >> 24) | 0xE0, 0x1F6);
            outb(0x20, 0x1F7);  /* cmd 0x20 - read sectors */
            while((inb(0x1F7) & 0xC0) != 0x40);
            insl(0x1F0, dst, 512/4);
        }
    }
#endif
}

/**
 * Get the next cluster from FAT
 */
uint32_t nextclu(uint32_t clu)
{
    uint64_t i;

    if(clu < 2 || clu >= 0x0FFFFFF8) return 0;
    if(clu < fat_cache || clu > fat_cache + 1023) {
        fat_cache = clu & ~1023;
        for(i = 0; i < 8; i++) loadsec(fat_lba + (fat_cache >> 7) + i, &fat[i << 7]);
    }
    clu = fat[clu - fat_cache];
    return clu < 2 || clu >= 0x0FFFFFF8 ? 0 : clu;
}

/**
 * Allocate and zero out a page
 */
uint64_t page_alloc(void)
{
    uint64_t page = page_buf;
    if(page == 0x20000) return 0;
    page_buf += 4096;
    memset((void*)(uintptr_t)page, 0, 4096);
    return page;
}

/**
 * Initialize firmware related stuff
 */
void fw_init(void)
{
    guid_t espGuid = EFI_PART_TYPE_EFI_SYSTEM_PART_GUID;
    uint64_t i, j, k, l, n;

    console_init();
#if CONFIG(LP_ARCH_X86)
    /* make sure SSE is enabled, because some say there are buggy firmware in the wild not enabling (and also needed if we come
     * from boot_x86.asm). No supported check, because according to AMD64 Spec Vol 2, all long mode capable CPUs must also
     * support SSE2 at least. We don't need them, but it's more than likely that a kernel is compiled using SSE instructions. */
    __asm__ __volatile__ (
    "movl %%cr0, %%eax;andb $0xF1, %%al;movl %%eax, %%cr0;"     /* clear MP, EM, TS (FPU emulation off) */
    "movl %%cr4, %%eax;orw $3 << 9, %%ax;movl %%eax, %%cr4;"    /* set OSFXSR, OSXMMEXCPT (enable SSE) */
    :::"rax");
    /* ridiculous, libpayload is a huge library, yet has no function to initialize serial port properly... */
    __asm__ __volatile__(
        "movl %0, %%edx;"
        "xorb %%al, %%al;outb %%al, %%dx;"               /* IER int off */
        "movb $0x80, %%al;addb $2,%%dl;outb %%al, %%dx;" /* LCR set divisor mode */
        "movb $1, %%al;subb $3,%%dl;outb %%al, %%dx;"    /* DLL divisor lo 115200 */
        "xorb %%al, %%al;incb %%dl;outb %%al, %%dx;"     /* DLH divisor hi */
        "incb %%dl;outb %%al, %%dx;"                     /* FCR fifo off */
        "movb $0x43, %%al;incb %%dl;outb %%al, %%dx;"    /* LCR 8N1, break on */
        "movb $0x8, %%al;incb %%dl;outb %%al, %%dx;"     /* MCR Aux out 2 */
        "xorb %%al, %%al;subb $4,%%dl;inb %%dx, %%al"    /* clear receiver/transmitter */
    : : "a"(CONFIG_LP_SERIAL_IOBASE + 1): );
#endif
    storage_initialize();
#if CONFIG(LP_USB)
    usb_initialize();
    usbdev = NULL;
    usb_poll(); /* this calls usbdisk_create() if it detects any USB storages */
#endif
    video_console_clear();

    /* initialize everything to zero */
#ifdef DEBUG
    verbose = 3;
#else
    verbose = 0;
#endif
    bpb = NULL; memmap = NULL; initrd = NULL; num_memmap = bkp = rq = smp = 0;
    zero_page = NULL;
    conf_buf = kernel = cmdline = NULL; kernel_entry = logo_buf = tags_ptr = NULL; kernel_mode = MODE_MB64;
    root_dir = rsdp_ptr = dsdt_ptr = ram = 0;
    memset(&vidmode, 0, sizeof(vidmode)); fb_bg = 0; pb_fb = NULL;
    vidmode.framebuffer_addr = (uint64_t)lib_sysinfo.framebuffer.physical_address;
    vidmode.framebuffer_pitch = lib_sysinfo.framebuffer.bytes_per_line;
    vidmode.framebuffer_width = lib_sysinfo.framebuffer.x_resolution;
    vidmode.framebuffer_height = lib_sysinfo.framebuffer.y_resolution;
    vidmode.framebuffer_bpp = lib_sysinfo.framebuffer.bits_per_pixel;
    vidmode.framebuffer_type = 1;
    vidmode.framebuffer_red_field_position = lib_sysinfo.framebuffer.red_mask_pos;
    vidmode.framebuffer_red_mask_size = lib_sysinfo.framebuffer.red_mask_size;
    vidmode.framebuffer_green_field_position = lib_sysinfo.framebuffer.green_mask_pos;
    vidmode.framebuffer_green_mask_size = lib_sysinfo.framebuffer.green_mask_pos;
    vidmode.framebuffer_blue_field_position = lib_sysinfo.framebuffer.blue_mask_pos;
    vidmode.framebuffer_blue_mask_size = lib_sysinfo.framebuffer.blue_mask_size;
    /* things with fixed address */
    tags_buf = (uint8_t*)0x20000; file_buf = 0x100000;
    pt = (uint64_t*)0x1000; page_buf = 0x9000;
    memset(pt, 0, 8 * 4096);
    /* the GUDT format is strongly preferred here, however this actually could be an FDT or a DSDT blob as well,
     * if the magic bytes match, it will be loaded and will work. Can be overriden by using the module directive */
    dsdt_ptr = (uint64_t)(uintptr_t)_cbfs_load("devices", NULL, NULL, 0);
#if CONFIG(LP_ARCH_ARM64)
    __asm__ __volatile__("mrs %0, midr_el1;":"=r"(mmio_base)::);
    switch(mmio_base & 0xFFF0) {
        case 0xD030: rpi = 3; mmio_base = 0x3F000000; emmc_base = 0x3F300000; break;     /* Raspberry Pi 3 */
        default:     rpi = 4; mmio_base = 0xFE000000; emmc_base = 0xFE340000; break;     /* Raspberry Pi 4 */
    }
    /* set up paging */
    j = mmio_base >> 21; k = (mmio_base + 0x800000) >> 21;
    /* TTBR0 */
    for(i = 0; i < 4; i++)
        pt[i] = (uintptr_t)pt + ((i + 2) * 4096) + (3|(3<<8)|(1<<10));
    for(i = 0; i < 4 * 512; i++) pt[1024 + i] = (uintptr_t)i * 2 * 1024 * 1024 +
        /* if we're mapping the mmio area, use device SH=2 memory type */
        (1 | (1<<10) | (i >= j && i < k ? (2<<8) | (1<<2) | (1L<<54) : (3<<8)));
    /* dynamically map framebuffer */
    if(vidmode.framebuffer_addr) {
        /* map new pages from the page table area */
        fw_map(vidmode.framebuffer_addr, vidmode.framebuffer_addr,
            (vidmode.framebuffer_pitch * vidmode.framebuffer_height + 4095) & ~4095);
    }
    /* TTBR1 */
    /* pt[512] = nothing for now; */
#endif
#if CONFIG(LP_ARCH_X86)
    /* set up the same page tables we have with BIOS, but don't active it yet */
    pt[0] = (uintptr_t)pt + 4096 + 3;
    for(i = 0; i < 5; i++) pt[512 + i] = (uintptr_t)pt + (i + 2) * 4096 + 3;
    for(i = 0; i < 6 * 512; i++) pt[1024 + i] = (uint64_t)i * 2 * 1024 * 1024 + 0x83;
#endif

    /* get boot partition's root directory */
    loadsec(1, &vbr);
    if(!memcmp(&vbr, EFI_PTAB_HEADER_ID, 8)) {
        /* found GPT */
        j = ((gpt_header_t*)&vbr)->SizeOfPartitionEntry;
        l = ((gpt_header_t*)&vbr)->PartitionEntryLBA;
        n = ((gpt_header_t*)&vbr)->NumberOfPartitionEntries;
        /* look for ESP in the first 8 sectors only. Should be the very first entry anyway */
        for(k = 0; k < 8 && n; k++) {
            loadsec(l + k, &vbr);
            for(i = 0; i + j <= 512; i += j, n--) {
                /* does ESP type match? */
                if(!root_dir && !memcmp(&((gpt_entry_t*)&vbr[i])->PartitionTypeGUID, &espGuid, sizeof(guid_t))) {
                    root_dir = ((gpt_entry_t*)&vbr[i])->StartingLBA;
                    memcpy(&bootuuid, &(((gpt_entry_t*)&vbr[i])->UniquePartitionGUID), sizeof(guid_t));
                    k = 8; break;
                }
            }
        }
    } else {
        /* fallback to MBR partitioning scheme */
        loadsec(0, &vbr);
        if(vbr[510] == 0x55 && vbr[511] == 0xAA)
            for(i = 0x1c0; i < 510; i += 16)
                if(vbr[i - 2] == 0x80/*active*/ && (vbr[i + 2] == 0xC/*FAT32*/ || vbr[i + 2] == 0xEF/*ESP*/)) {
                    root_dir = (uint64_t)(*((uint32_t*)&vbr[i + 6]));
                    memcpy(&bootuuid.Data1, "PART", 4); memcpy(bootuuid.Data4, "boot", 4);
                    bootuuid.Data3 = (i - 0x1c0) / 16;
                    break;
                }
    }
    if(root_dir) {
        loadsec(root_dir, &vbr);
        bpb = (esp_bpb_t*)&vbr;
        if(vbr[510] != 0x55 || vbr[511] != 0xAA || bpb->bps != 512 || !bpb->spc || bpb->spf16 || !bpb->spf32)
            root_dir = 0;
        else {
            /* calculate the LBA address of the FAT and the first data sector */
            fat_lba = bpb->rsc + root_dir;
            data_lba = bpb->spf32 * bpb->nf + bpb->rsc - 2 * bpb->spc + root_dir;
            /* load the beginning of the FAT into the cache */
            for(i = 0; i < 8; i++) loadsec(fat_lba + i, &fat[i << 7]);
            fat_cache = 0;
        }
    }
}

/**
 * Display a boot splash
 */
void fw_bootsplash(void)
{
    uint32_t c;
    uint8_t *fb = (uint8_t*)(uintptr_t)vidmode.framebuffer_addr;
    int i, j, k, l, x, y, w, h, o, m, p, px, py, b = (vidmode.framebuffer_bpp + 7) >> 3;

    /* clear screen */
    video_console_clear();
    if(!fb) return;
    for(j = y = 0; y < (int)vidmode.framebuffer_height; y++, j += vidmode.framebuffer_pitch)
        for(i = j, x = 0; x < (int)vidmode.framebuffer_width; x++, i += b)
            if(b == 2) *((uint16_t*)(fb + i)) = (uint16_t)fb_bg; else *((uint32_t*)(fb + i)) = fb_bg;
    /* only indexed RLE compressed TGA images supported */
    if(!logo_buf || logo_buf[0] || logo_buf[1] != 1 || logo_buf[2] != 9 || logo_buf[3] || logo_buf[4] ||
        (logo_buf[7] != 24 && logo_buf[7] != 32)) return;
    /* uncompress image */
    o = (logo_buf[17] & 0x20); w = (logo_buf[13] << 8) + logo_buf[12]; h = (logo_buf[15] << 8) + logo_buf[14];
    if(w < 1 || h < 1) return;
    px = ((int)vidmode.framebuffer_width - w) / 2; py = ((int)vidmode.framebuffer_height - h) / 2;
    m = ((logo_buf[7] >> 3) * ((logo_buf[6] << 8) | logo_buf[5])) + 18; y = i = 0;
    for(l = 0, x = w, y = -1; l < w * h && (uint32_t)m < logo_size;) {
        k = logo_buf[m++];
        if(k > 127) { p = 0; k -= 127; j = logo_buf[m++] * (logo_buf[7] >> 3) + 18; } else { p = 1; k++; }
        l += k;
        while(k--) {
            if(p) j = logo_buf[m++] * (logo_buf[7] >> 3) + 18;
            if(x == w) { x = 0; i = ((py + (!o ? h - y - 1 : y)) * (int)vidmode.framebuffer_pitch + (px + x) * b); y++; }
            if(py + y > 0 && py + y < (int)vidmode.framebuffer_height - 1) {
                if(px + x > 0 && px + x < (int)vidmode.framebuffer_width - 1) {
                    c = FB_COLOR(logo_buf[j + 2], logo_buf[j + 1], logo_buf[j + 0]);
                    if(b == 2) *((uint16_t*)(fb + i)) = (uint16_t)c; else *((uint32_t*)(fb + i)) = c;
                }
                i += b;
            }
            x++;
        }
    }
}

/**
 * Open a file
 */
int fw_open(char *fn)
{
    uint64_t lba;
    uint32_t clu = bpb->rc;
    int i, n = 0, m = 0;
    uint8_t secleft = 0, *dir = data + sizeof(data);
    uint16_t *u, *s = wcname, a, b, c, *d;
    char *e;

    if(!root_dir || !fn || !*fn) return 0;
    /* UTF-8 to WCHAR */
    for(e = fn, d = wcname; *e && *e != ' ' && *e != '\r' && *e != '\n' && d < &wcname[PATH_MAX - 2]; d++) {
        if((*e & 128) != 0) {
            if(!(*e & 32)) { c = ((*e & 0x1F)<<6)|(*(e+1) & 0x3F); e++; } else
            if(!(*e & 16)) { c = ((*e & 0xF)<<12)|((*(e+1) & 0x3F)<<6)|(*(e+2) & 0x3F); e += 2; } else
            if(!(*e & 8)) { c = ((*e & 0x7)<<18)|((*(e+1) & 0x3F)<<12)|((*(e+2) & 0x3F)<<6)|(*(e+3) & 0x3F); *e += 3; }
            else c = 0;
        } else c = *e;
        e++; if(c == '\\' && *e == ' ') { c = ' '; e++; }
        *d = c;
    }
    *d = 0;

    file_size = file_clu = 0;
    memset(lfn, 0, sizeof(lfn));
    while(1) {
        /* have we reached the end of the sector? */
        if(dir >= data + sizeof(data)) {
            if(secleft) { secleft--; lba++; }
            else {
                if(clu < 2 || clu >= 0x0FFFFFF8) return 0;
                secleft = bpb->spc - 1;
                lba = clu * bpb->spc + data_lba;
                clu = nextclu(clu);
            }
            loadsec(lba, &data);
            dir = data;
        }
        /* empty space? End of directory then */
        if(!dir[0]) return 0;
        /* not a deleted entry or current and parent entries? */
        if(dir[0] != 5 && dir[0] != 0xE5 && (dir[0] != '.' || (dir[1] != '.' && dir[1] != ' '))) {
            /* is this an LFN block? */
            if(dir[0xB] == 0xF) {
                /* first LFN block? */
                if(!n || (dir[0] & 0x40)) {
                    memset(lfn, 0, sizeof(lfn));
                    n = dir[0] & 0x1F;
                    /* bad record, not sure what to do. Let's reset state and continue with next entry */
                    if(n < 1 || n > 20) { n = m = 0; dir += 32; continue; }
                    u = lfn + (n - 1) * 13;
                }
                /* get the next part of UCS-2 characters */
                for(i = 0; i < 5; i++)
                    u[i] = dir[i*2+2] << 8 | dir[i*2+1];
                for(i = 0; i < 6; i++)
                    u[i+5] = dir[i*2+0xF] << 8 | dir[i*2+0xE];
                u[11] = dir[0x1D] << 8 | dir[0x1C];
                u[12] = dir[0x1F] << 8 | dir[0x1E];
                u -= 13;
                n--;
                /* indicate that the next directory entry belongs to an LFN */
                m = (!n && u < lfn);
            } else
            if(!(dir[0xB] & 8)) {
                /* if we don't have an LFN already, generate it from the 8.3 name in this entry */
                if(!m) {
                    for(i = 0; i < 8; i++) lfn[i] = dir[i];
                    while(i && lfn[i - 1] == ' ') i--;
                    if(dir[8] != ' ') {
                        lfn[i++] = '.'; lfn[i++] = dir[8];
                        if(dir[9] != ' ') {
                            lfn[i++] = dir[9];
                            if(dir[10] != ' ') { lfn[i++] = dir[10]; }
                        }
                    }
                    lfn[i] = 0;
                } else m = 0;
                /* filename match? */
                if(*s == '/') s++;
                for(i = 0; lfn[i] && s[i] && s[i] != '/'; i++) {
                    a = lfn[i]; if(a >= 'A' && a <= 'Z') a += 'a' - 'A';
                    b = s[i]; if(b >= 'A' && b <= 'Z') b += 'a' - 'A';
                    if(a != b) break;
                }
                if(!lfn[i]) {
                    clu = (dir[0x15] << 24) | (dir[0x14] << 16) | (dir[0x1B] << 8) | dir[0x1A];
                    /* is this a directory? */
                    if(dir[0xB] & 0x10) {
                        if(s[i] != '/') return 0;
                        /* go to subdirectory */
                        s += i + 1; n = m = secleft = 0; dir = data + sizeof(data);
                        continue;
                    } else {
                        /* no, it's a file, then we have located what we were looking for */
                        if(clu < 2 || clu >= 0x0FFFFFF8) return 0;
                        file_clu = clu;
                        file_size = (dir[0x1F] << 24) | (dir[0x1E] << 16) | (dir[0x1D] << 8) | dir[0x1C];
                        break;
                    }
                }
            }
        }
        dir += 32;
    }
    return 1;
}

/**
 * Read data from file
 */
uint64_t fw_read(uint64_t offs, uint64_t size, void *buf)
{
    uint64_t lba = 0, rem, o;
    uint32_t clu = file_clu, nc, ns = 0, os = 0, rs = 512;
    uint8_t secleft = 0;

    if(!root_dir || file_clu < 2 || offs >= file_size || !size || !buf) return 0;
    if(offs + size > file_size) size = file_size - offs;
    rem = size;

    pb_init(size);
    if(offs) {
        nc = offs / (bpb->spc << 9); o = offs % (bpb->spc << 9);
        ns = o >> 9; os = o & 0x1ff; rs = 512 - os;
        if(nc) { while(nc-- && clu) { clu = nextclu(clu); } if(!clu) return 0; }
        secleft = bpb->spc - ns - 1;
        lba = clu * bpb->spc + ns - 1 + data_lba;
    }
    while(rem && !rq) {
        if(secleft) { secleft--; lba++; }
        else {
            if(!clu) break;
            secleft = bpb->spc - 1;
            lba = clu * bpb->spc + data_lba;
            clu = nextclu(clu);
        }
        if(rs > rem) rs = rem;
        if(rs < 512) {
            loadsec(lba, data);
            memcpy(buf, data + os, rs); os = 0;
        } else {
            loadsec(lba, buf);
            if(os) { memcpy(buf, buf + os, rs); os = 0; }
        }
        buf += rs; rem -= rs; rs = 512;
        pb_draw(size - rem);
    }
    pb_fini();
    return (size - rem);
}

/**
 * Close file
 */
void fw_close(void)
{
    file_clu = 0;
}

/**
 * Load and parse config (everything except modules)
 */
void fw_loadconfig(void)
{
    char *s, *e, *a;
    uint32_t r, g, b;
    int l, m = 0;

    if(bkp) { fw_bootsplash(); printf("Aborted, loading backup configuration...\r\n"); }

    kernel = cmdline = NULL;
    tags_buf = (uint8_t*)0x20000;
    logo_size = 0;
    if(!conf_buf) {
        file_size = 0;
        /* first check if there's a config in the ROM */
        r = cbfs_get_size("simpleboot.cfg");
        if(r) file_size = cbfs_load("simpleboot.cfg", tags_buf, r);
        /* as a fallback, we try to load the first menuentry from easyboot's configuration */
        else if(fw_open("simpleboot.cfg") || (!bkp && fw_open("easyboot/menu.cfg"))) {
            fw_read(0, file_size, tags_buf);
            fw_close();
        }
        tags_buf[file_size] = 0;
        if(file_size) {
            conf_buf = (char*)tags_buf;
            tags_buf += (file_size + 7) & ~7;
        }
    }
    if(conf_buf) {
        fb_bg = 0; smp = 0;
        for(s = conf_buf; *s;) {
            /* find beginning of a line */
            while(*s && (*s == '\r' || *s == '\n' || *s == ' ' || *s == '\t')) s++;
            for(a = s; *a && *a != ' ' && *a != '\r' && *a != '\n'; a++);
            for(e = a; *e && *e != '\r' && *e != '\n'; e++);
            for(; a < e && *a == ' '; a++);
            /* 's' points to the start of the command,
             * 'a' to the first argument,
             * 'e' to the end of the line */
            l = !memcmp(s, "backup", 6);
            if(bkp ^ l) { s = e; continue; } else if(bkp & l) s += 6;
            if(!memcmp(s, "multicore", 9)) smp = 1;
            if(a >= e) { s = e; continue; }
            if(!memcmp(s, "menuentry", 9)) {
                if(++m > 1) break;
            } else
            if(!memcmp(s, "verbose", 7)) {
                a = getint(a, &verbose);
            } else
            if(!memcmp(s, "framebuffer", 11)) {
                /* we can't change resolution with libpayload */
            } else
            if(!memcmp(s, "bootsplash", 10)) {
                if(*a == '#') {
                    a++; a = gethex(a, &r); a = gethex(a, &g); a = gethex(a, &b);
                    fb_bg = FB_COLOR(r, g, b);
                    while(a < e && *a == ' ') a++;
                }
                if(a < e) {
                    if(fw_open(a)) {
                        logo_buf = tags_buf;
                        tags_buf += (file_size + 7) & ~7;
                        if(verbose) printf("Loading logo (%llu bytes)...\r\n", file_size);
                        logo_size = file_size;
                        fw_read(0, file_size, logo_buf);
                        fw_close();
                    } else logo_size = 0;
                }
                fw_bootsplash();
            } else
            if(a < e && !memcmp(s, "kernel", 6)) {
                kernel = a;
                for(; a < e && *a && *a != ' ' && *a != '\r' && *a != '\n'; a++)
                    if(*a == '\\' && a[1] == ' ') a++;
                while(a < e && *a == ' ') a++;
                if(*a && *a != '\r' && *a != '\n') cmdline = a;
            }
            /* go to the next line */
            s = e;
        }
    }
    if(!logo_size && (r = cbfs_get_size("logo.tga"))) {
        logo_size = cbfs_load("logo.tga", tags_buf, r);
        logo_buf = tags_buf;
        tags_buf += (logo_size + 7) & ~7;
        fw_bootsplash();
    }
}

/**
 * Detect config file independent configuration and generate tags for them
 */
void fw_loadsetup()
{
    multiboot_tag_loader_t *stag;
    multiboot_tag_mmap_t *mtag;
    multiboot_mmap_entry_t tmp;
    uint64_t srt, end;
    uint32_t i, j;
    char *c;

    file_buf = 0x100000;
    tags_ptr = tags_buf;
    memmap = NULL;
    if(tags_ptr) {
        /* MBI header */
        ((multiboot_info_t*)tags_buf)->total_size = ((multiboot_info_t*)tags_buf)->reserved = 0;
        tags_ptr += sizeof(multiboot_info_t);
        /* loader tag */
        stag = (multiboot_tag_loader_t*)tags_ptr;
        stag->type = MULTIBOOT_TAG_TYPE_BOOT_LOADER_NAME;
        stag->size = bkp ? 28 : 19;
        memcpy(stag->string, SIMPLEBOOT_MAGIC, 11);
        if(bkp) memcpy(stag->string + 10, " (backup)", 10);
        tags_ptr += (stag->size + 7) & ~7;
        /* commandline tag */
        if(cmdline) {
            for(c = cmdline; *c && *c != '\r' && *c != '\n'; c++);
            stag = (multiboot_tag_loader_t*)tags_ptr;
            stag->type = MULTIBOOT_TAG_TYPE_CMDLINE;
            stag->size = 9 + c - cmdline;
            memcpy(stag->string, cmdline, c - cmdline); stag->string[c - cmdline] = 0;
            tags_ptr += (stag->size + 7) & ~7;
            /* overwrite the cmdline pointer with this new, zero terminated string */
            cmdline = stag->string;
        }
        /* get system tables and generate tags for them */
        if((c = (char*)lib_sysinfo.smbios)) {
            memset(tags_ptr, 0, sizeof(multiboot_tag_smbios_t));
            ((multiboot_tag_smbios_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_SMBIOS;
            ((multiboot_tag_smbios_t*)tags_ptr)->size = sizeof(multiboot_tag_smbios_t) + (uint32_t)c[5];
            ((multiboot_tag_smbios_t*)tags_ptr)->major = c[7];
            ((multiboot_tag_smbios_t*)tags_ptr)->minor = c[8];
            memcpy(((multiboot_tag_smbios_t*)tags_ptr)->tables, c, (uint32_t)c[5]);
            tags_ptr += (((multiboot_tag_smbios_t*)tags_ptr)->size + 7) & ~7;
        }
        if((c = (char*)lib_sysinfo.acpi_rsdp)) {
            if(c[15] < 2) {
                ((multiboot_tag_old_acpi_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_ACPI_OLD;
                ((multiboot_tag_old_acpi_t*)tags_ptr)->size = sizeof(multiboot_tag_old_acpi_t) + 24;
                memcpy(((multiboot_tag_old_acpi_t*)tags_ptr)->rsdp, c, 24);
                tags_ptr += (((multiboot_tag_old_acpi_t*)tags_ptr)->size + 7) & ~7;
            } else {
                ((multiboot_tag_new_acpi_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_ACPI_NEW;
                ((multiboot_tag_new_acpi_t*)tags_ptr)->size = sizeof(multiboot_tag_new_acpi_t) + 36;
                memcpy(((multiboot_tag_new_acpi_t*)tags_ptr)->rsdp, c, 36);
                tags_ptr += (((multiboot_tag_new_acpi_t*)tags_ptr)->size + 7) & ~7;
            }
            rsdp_ptr = (uintptr_t)c;
        }
        /* generate memory map tag */
        mtag = (multiboot_tag_mmap_t*)tags_ptr;
        mtag->type = MULTIBOOT_TAG_TYPE_MMAP;
        mtag->entry_size = sizeof(multiboot_mmap_entry_t);
        mtag->entry_version = 0;
        for(ram = num_memmap = i = 0; (int)i < lib_sysinfo.n_memranges; i++) {
            srt = lib_sysinfo.memrange[i].base;
            end = srt + lib_sysinfo.memrange[i].size;
            srt = (srt + 4095) & ~4095; end &= ~4095;
            if(srt < end) {
                mtag->entries[num_memmap].base_addr = srt;
                mtag->entries[num_memmap].length = end - srt;
                mtag->entries[num_memmap].reserved = lib_sysinfo.memrange[i].type << 16;
                mtag->entries[num_memmap].type =
                    lib_sysinfo.memrange[i].type == CB_MEM_RAM || lib_sysinfo.memrange[i].type == CB_MEM_TABLE ? MULTIBOOT_MEMORY_AVAILABLE : (
                    lib_sysinfo.memrange[i].type == CB_MEM_ACPI ? MULTIBOOT_MEMORY_ACPI_RECLAIMABLE : (
                    lib_sysinfo.memrange[i].type == CB_MEM_NVS ? MULTIBOOT_MEMORY_NVS : MULTIBOOT_MEMORY_RESERVED));
                if(mtag->entries[num_memmap].type == MULTIBOOT_MEMORY_AVAILABLE && end - 1 > ram) ram = end - 1;
                num_memmap++;
            }
        }
        if(num_memmap > 0) {
            /* make sure of it that the memory map is sorted. Should be, so bubble-sort is affordable here */
            for(i = 1; i < num_memmap; i++)
                for(j = i; j > 0 && mtag->entries[j].base_addr < mtag->entries[j - 1].base_addr; j--) {
                    memcpy(&tmp, &mtag->entries[j - 1], sizeof(multiboot_mmap_entry_t));
                    memcpy(&mtag->entries[j - 1], &mtag->entries[j], sizeof(multiboot_mmap_entry_t));
                    memcpy(&mtag->entries[j], &tmp, sizeof(multiboot_mmap_entry_t));
                }
            memmap = mtag->entries;
            mtag->size = sizeof(multiboot_tag_mmap_t) + num_memmap * sizeof(multiboot_mmap_entry_t);
            tags_ptr += (mtag->size + 7) & ~7;
        }
        ram &= ~(2 * 1024 * 1024 - 1);
    }
}

/**
 * Parse config for modules and load them
 */
void fw_loadmodules(void)
{
    uint32_t uncomp;
    uint8_t *ptr, tmp[16];
    int n = 0, f = 0, l;
    multiboot_tag_module_t *tag;
    char *s, *e, *a;

    if(conf_buf) {
        for(s = conf_buf; !rq && !f && *s;) {
            /* find beginning of a line */
            while(*s && (*s == '\r' || *s == '\n' || *s == ' ' || *s == '\t')) s++;
            for(a = s; *a && *a != ' ' && *a != '\r' && *a != '\n'; a++);
            for(e = a; *e && *e != '\r' && *e != '\n'; e++);
            for(; a < e && *a == ' '; a++);
            /* 's' points to the start of the command,
             * 'a' to the first argument,
             * 'e' to the end of the line */
            l = !memcmp(s, "backup", 6);
            if(a >= e || bkp ^ l) { s = e; continue; } else if(bkp & l) s += 6;
            if(!memcmp(s, "module", 6)) {
ldinitrd:       if(fw_open(a)) {
                    fw_read(0, 16, (void*)tmp);
                    /* if it's a gzip compressed module, then load it after bss, and uncompress to position */
                    uncomp = 0;
                    if(tmp[0] == 0x1f && tmp[1] == 0x8b)
                        fw_read(file_size - 4, 4, (void*)&uncomp);
                    else
                    if(tmp[0] == 'G' && tmp[1] == 'U' && tmp[2] == 'D' && tmp[8] == 0x78)
                        uncomp = (((tmp[4] | (tmp[5] << 8)) + 7) & ~7) + ((tmp[6] | (tmp[7] << 8)) << 4);
                    ptr = (uint8_t*)(uintptr_t)file_buf;
                    if(verbose) printf("Loading module (%llu bytes)...\r\n", file_size);
                    fw_read(0, file_size, (void*)(uncomp ? (uintptr_t)&_end : (uintptr_t)file_buf));
                    if(uncomp) {
                        if(verbose) printf("Uncompressing (%u bytes)...\r\n", uncomp);
                        uncompress((uint8_t*)&_end, (uint8_t*)(uintptr_t)file_buf, uncomp);
                        file_size = uncomp;
                    }
                    file_buf += (file_size + 4095) & ~4095;
                    /* if it's a DTB or a DSDT, don't add it to the modules list, add it to the ACPI tables */
                    if(ptr[0] == 0xD0 && ptr[1] == 0x0D && ptr[2] == 0xFE && ptr[3] == 0xED) {
                        if(verbose) printf("DTB detected...\r\n");
                        dsdt_ptr = (uint64_t)(uintptr_t)ptr;
                    } else
                    if(((ptr[0] == 'D' && ptr[1] == 'S') || (ptr[0] == 'G' && ptr[1] == 'U')) && ptr[2] == 'D' && ptr[3] == 'T') {
                        if(verbose) printf("%c%cDT detected...\n", ptr[0], ptr[1]);
                        dsdt_ptr = (uint64_t)(uintptr_t)ptr;
                    } else {
                        if(tags_ptr) {
                            tag = (multiboot_tag_module_t*)tags_ptr;
                            tag->type = MULTIBOOT_TAG_TYPE_MODULE;
                            tag->size = sizeof(multiboot_tag_module_t) + e - a + 1;
                            tag->mod_start = (uint32_t)(uintptr_t)ptr;
                            tag->mod_end = file_buf;
                            memcpy(tag->string, a, e - a); tag->string[e - a] = 0;
                            if(verbose > 2) hexdump(ptr, 1);
                            tags_ptr += (tag->size + 7) & ~7;
                            if(!initrd) initrd = tag;
                        }
                        n++;
                    }
                    fw_close();
                }
            }
            /* go to the next line */
            s = e;
        }
    }
    /* if no modules were loaded, but we have a default initrd name, try to add that */
    if(!n && !f) { f = 1; a = bkp ? "initramfs-linux-fallback.img" : "initramfs-linux.img"; for(e = a; *e; e++){} goto ldinitrd; }
#if CONFIG(LP_ARCH_X86)
    if(!n && f == 1) { f = 2; a = bkp ? "ibmpc/initrd.bak" : "ibmpc/initrd"; e = a + (bkp ? 16 : 12); goto ldinitrd; }
#endif
    /* if still no modules were loaded, but we have a default initrd embedded, try to add that */
    if(!n && f > 0 && !initrd && tags_ptr && (file_size = cbfs_get_size("initrd"))) {
        if(verbose) printf("Loading module (%llu bytes)...\r\n", file_size);
        ptr = (uint8_t*)(uintptr_t)file_buf;
        file_size = cbfs_load("initrd", ptr, file_size);
        if(ptr[0] == 0x1f && ptr[1] == 0x8b) {
            memcpy(&uncomp, ptr + file_size - 4, 4);
            memcpy(ptr + uncomp + 4096, ptr, file_size);
            if(verbose) printf("Uncompressing (%u bytes)...\r\n", uncomp);
            uncompress(ptr + uncomp + 4096, ptr, uncomp);
            file_size = uncomp;
        }
        file_buf = (file_buf + file_size + 4095) & ~4095;
        tag = initrd = (multiboot_tag_module_t*)tags_ptr;
        tag->type = MULTIBOOT_TAG_TYPE_MODULE;
        tag->size = sizeof(multiboot_tag_module_t) + 7 + 1;
        tag->mod_start = (uint32_t)(uintptr_t)ptr;
        tag->mod_end = file_buf;
        memcpy(tag->string, "initrd", 7);
        if(verbose > 2) hexdump(ptr, 1);
        tags_ptr += (tag->size + 7) & ~7;
    }
}

/**
 * Map virtual memory
 */
int fw_map(uint64_t phys, uint64_t virt, uint32_t size)
{
    uint64_t end = virt + size, *ptr, *next = NULL, orig = page_buf;

    /* is this a canonical address? We handle virtual memory up to 256TB */
    if(!pt || ((virt >> 48L) != 0x0000 && (virt >> 48L) != 0xffff)) return 0;

    /* walk the page tables and add the missing pieces */
    for(virt &= ~4095, phys &= ~4095; virt < end; virt += 4096) {
        /* 512G */
        ptr = &pt[(virt >> 39L) & 511];
        if(!*ptr) { if(!(*ptr = page_alloc())) return 0; else *ptr |= 3; }
        /* 1G */
        ptr = (uint64_t*)(uintptr_t)(*ptr & ~4095); ptr = &ptr[(virt >> 30L) & 511];
        if(!*ptr) { if(!(*ptr = page_alloc())) return 0; else *ptr |= 3; }
        /* 2M if we previously had a large page here, split it into 4K pages */
        ptr = (uint64_t*)(uintptr_t)(*ptr & ~4095); ptr = &ptr[(virt >> 21L) & 511];
        if(!*ptr || *ptr & 0x80) { if(!(*ptr = page_alloc())) return 0; else *ptr |= 3; }
        /* 4K */
        ptr = (uint64_t*)(uintptr_t)(*ptr & ~4095); ptr = &ptr[(virt >> 12L) & 511];
        /* if this page is already mapped, that means the kernel has invalid, overlapping segments */
        if(!*ptr) { *ptr = (uint64_t)(uintptr_t)next; next = ptr; }
    }
    /* resolve the linked list */
    for(end = ((phys == orig ? page_buf : phys) + size - 1) & ~4095; next; end -= 4096, next = ptr) {
        ptr = (uint64_t*)(uintptr_t)*next; *next = end | 3;
    }
    return 1;
}

/**
 * Load a kernel segment
 */
int fw_loadseg(uint32_t offs, uint32_t filesz, uint64_t vaddr, uint32_t memsz)
{
    uint64_t top;
    uint8_t *buf = (uint8_t*)(uintptr_t)vaddr;
    uint32_t size, i;

    if(!memsz || !file_size) return 1;
    if(verbose > 1) printf("  segment %08x[%08x] -> %08llx[%08x]\r\n", offs, filesz, vaddr, memsz);
    size = (memsz + (vaddr & 4095) + 4095) & ~4095;
    /* no overwriting of the loader data */
    if(vaddr < 0x20000 + (TAGS_MAX + 2) * 4096 || (vaddr > 0x300000 && vaddr < 0x400000)) goto err;
    if(vaddr > ram) {
        /* possibly a higher-half kernel's segment, we must map it */
        if(!fw_map(file_buf, vaddr, size)) goto err;
        buf = (void*)(uintptr_t)file_buf; file_buf += size;
    } else {
        /* make sure we load modules after the kernel to avoid any conflict */
        top = ((uintptr_t)buf + size + 4095) & ~4095; if(file_buf < top) file_buf = top;
        /* let's see if the memory where the segment wants to be loaded is free or not */
        for(i = 0; memmap && i < num_memmap; i++)
            /* find which memory slot it fits */
            if(memmap[i].base_addr <= vaddr && memmap[i].base_addr + memmap[i].length > vaddr) {
                /* if that slot isn't free or the end doesn't fit in the slot too, then that's a problem */
                if(memmap[i].type != MULTIBOOT_MEMORY_AVAILABLE ||
                  memmap[i].base_addr + memmap[i].length < (vaddr & ~4095) + size)
                    goto err;
                break;
            }
        /* if no memory slots found, that's a dunno. Not used memory for sure, so let's try to load it, maybe works... */
    }
    if(filesz) {
        if(kernel_mem) memcpy(buf, kernel_mem + offs, filesz);
        else fw_read(offs, filesz, buf);
    }
    if(memsz > filesz) memset(buf + filesz, 0, memsz - filesz);
    return 1;
err:printf("ERROR: unable to load segment %08llx[%x], memory already in use\r\n", vaddr, memsz);
    return 0;
}

/**
 * Load the kernel
 */
int fw_loadkernel(void)
{
    void *p = (void*)kernel_buf;
    linux_boot_t *hdr = (linux_boot_t*)(kernel_buf + 0x1f1);
    pe_hdr *pe;
    pe_sec *sec;
    uint8_t *ptr;
    uint64_t offs;
    int i;

    kernel_mem = NULL; file_buf = 0x100000;
    /* do some heuristic on kernel name to allow multiple configurations */
    if((!kernel || !*kernel || !memcmp(kernel, "kernel", 6)) && (offs = cbfs_get_size("kernel"))) {
        kernel_mem = (uint8_t*)(uintptr_t)file_buf;
        file_size = cbfs_load("kernel", kernel_mem, offs);
        file_buf = (file_buf + file_size + 4095) & ~4095;
        memcpy(kernel_buf, kernel_mem, sizeof(kernel_buf));
    }
    if(!kernel_mem && (!kernel || !*kernel || !fw_open(kernel))) {
        if((!bkp || !fw_open("vmlinuz-fallback")) && !fw_open("vmlinuz-linux") && !fw_open("bzImage") && !fw_open("kernel")
#if CONFIG(LP_ARCH_X86)
          && !fw_open("ibmpc/core")
#endif
          )
            file_size = 0;
    }
    if(!file_size) {
        printf("ERROR: kernel not found\r\n");
        smp = 0;
        return 0;
    } else
    if(!kernel_mem)
        fw_read(0, sizeof(kernel_buf), p);
    /* we must check Linux before COFF/PE, because it might disguise itself as an EFI app */
#if CONFIG(LP_ARCH_X86)
    if(hdr->boot_flag == 0xAA55 && !memcmp(&hdr->header, HDRSMAG, 4)) {
        if(hdr->version < 0x20c || ((hdr->pref_address + file_size) >> 32L)) {
            printf("ERROR: unsupported Linux boot protocol version\r\n"); goto err;
        }
        /* it's a Linux kernel */
        kernel_mode = MODE_LIN; smp = 0;
        if(verbose) printf("Loading Linux kernel...\r\n");
        zero_page = (linux_boot_params_t*)0x90000;
        if(!hdr->setup_sects) hdr->setup_sects = 4;
        memset(zero_page, 0, sizeof(linux_boot_params_t));
        memcpy(&zero_page->hdr, hdr, 0x202 - 0x1f1 + kernel_buf[0x201]);
        zero_page->hdr.root_dev = 0x100; zero_page->hdr.root_flags = 1; zero_page->hdr.vid_mode = 0xffff;
        zero_page->hdr.type_of_loader = 0xff;
        /*zero_page->hdr.type_of_loader = 0xe0; zero_page->hdr.ext_loader_type = 0x14;*/
        if(cmdline) {
            ptr = (uint8_t*)zero_page + sizeof(linux_boot_params_t);
            zero_page->hdr.cmd_line_ptr = (uint32_t)(uintptr_t)ptr;
            for(i = 0; i < 32767 && cmdline[i] && cmdline[i] != '\r' && cmdline[i] != '\n'; i++) ptr[i] = cmdline[i];
            ptr[i] = 0;
        }
        if(!fw_loadseg((hdr->setup_sects + 1) * 512, hdr->init_size, hdr->pref_address, hdr->init_size)) goto err;
        kernel_entry = (uint8_t*)(uintptr_t)hdr->pref_address + 512;
    } else
    if(!memcmp(((Elf32_Ehdr*)p)->e_ident, ELFMAG, 4) &&
      (((Elf32_Ehdr*)p)->e_machine == EM_386 || ((Elf32_Ehdr*)p)->e_machine == EM_X86_64)) {
        /* it's a Multiboot2 ELF kernel */
        kernel_mode = ((Elf32_Ehdr*)p)->e_ident[EI_CLASS] == ELFCLASS64 ? MODE_MB64 : MODE_MB32;
        if(verbose) printf("Loading Multiboot2 ELF%d kernel...\r\n", kernel_mode == MODE_MB64 ? 64 : 32);
        if(kernel_mode == MODE_MB64) {
            kernel_entry = (uint8_t*)(uintptr_t)((Elf64_Ehdr*)p)->e_entry;
            ptr = p + ((Elf64_Ehdr*)p)->e_phoff;
            for(i = 0; !rq && i < ((Elf64_Ehdr*)p)->e_phnum && ptr + ((Elf64_Ehdr*)p)->e_phentsize < kernel_buf + sizeof(kernel_buf);
              i++, ptr += ((Elf64_Ehdr*)p)->e_phentsize)
                if(((Elf64_Phdr*)ptr)->p_type == PT_LOAD && !fw_loadseg(
                    (((Elf64_Phdr*)ptr)->p_offset), (((Elf64_Phdr*)ptr)->p_filesz),
                    (((Elf64_Phdr*)ptr)->p_vaddr), (((Elf64_Phdr*)ptr)->p_memsz))) goto err;
        } else {
            kernel_entry = (uint8_t*)(uintptr_t)((Elf32_Ehdr*)p)->e_entry;
            ptr = p + ((Elf32_Ehdr*)p)->e_phoff;
            for(i = 0; !rq && i < ((Elf32_Ehdr*)p)->e_phnum && ptr + ((Elf32_Ehdr*)p)->e_phentsize < kernel_buf + sizeof(kernel_buf);
              i++, ptr += ((Elf32_Ehdr*)p)->e_phentsize)
                if(((Elf32_Phdr*)ptr)->p_type == PT_LOAD && !fw_loadseg(
                    ((Elf32_Phdr*)ptr)->p_offset, ((Elf32_Phdr*)ptr)->p_filesz,
                    (uint64_t)((Elf32_Phdr*)ptr)->p_vaddr, ((Elf32_Phdr*)ptr)->p_memsz)) goto err;
        }
    } else
    if(((mz_hdr*)p)->magic == MZ_MAGIC && ((pe_hdr*)(p + ((mz_hdr*)p)->peaddr))->magic == PE_MAGIC &&
      (((pe_hdr*)(p + ((mz_hdr*)p)->peaddr))->machine == IMAGE_FILE_MACHINE_I386 ||
       ((pe_hdr*)(p + ((mz_hdr*)p)->peaddr))->machine == IMAGE_FILE_MACHINE_AMD64)) {
        /* it's a Multiboot2 COFF/PE kernel */
        pe = (pe_hdr*)(p + ((mz_hdr*)p)->peaddr);
        kernel_mode = pe->file_type == PE_OPT_MAGIC_PE32PLUS ? MODE_MB64 : MODE_MB32;
        offs = kernel_mode == MODE_MB64 ? (uint32_t)pe->data.pe64.img_base : pe->data.pe32.img_base;
        kernel_entry = offs + (uint8_t*)(uintptr_t)pe->entry_point;
        if(verbose) printf("Loading Multiboot2 PE%d kernel...\r\n", kernel_mode == MODE_MB64 ? 64 : 32);
        sec = (pe_sec*)((uint8_t*)pe + pe->opt_hdr_size + 24);
        for(i = 0; !rq && i < pe->sections && (uint8_t*)&sec[1] < kernel_buf + sizeof(kernel_buf); i++, sec++)
            if(!fw_loadseg(sec->raddr, sec->rsiz,
                /* the PE section vaddr field is only 32 bits, we must make sure that it properly sign extended to 64 bit */
                offs + (pe->file_type == PE_OPT_MAGIC_PE32PLUS ? (int64_t)(int32_t)sec->vaddr : sec->vaddr), sec->vsiz)) goto err;
    } else
#endif
#if CONFIG(LP_ARCH_ARM64)
    if(!memcmp(kernel_buf, "MZ", 2) && !memcmp(kernel_buf + 0x38, "ARM", 3) && kernel_buf[0x3b] == 64) {
        /* it's a Linux kernel */
        kernel_mode = MODE_LIN; smp = 0;
        if(verbose) printf("Loading Linux kernel...\r\n");
        if(!fw_loadseg(0, file_size, 0x80000, file_size)) goto err;
    } else
    if(!memcmp(((Elf64_Ehdr*)p)->e_ident, ELFMAG, 4) && ((Elf64_Ehdr*)p)->e_ident[EI_CLASS] == ELFCLASS64 &&
      ((Elf64_Ehdr*)p)->e_machine == EM_AARCH64) {
        /* it's a Multiboot2 ELF kernel */
        kernel_mode = MODE_MB64;
        kernel_entry = (uint8_t*)(uintptr_t)((Elf64_Ehdr*)p)->e_entry;
        if(verbose) printf("Loading Multiboot2 ELF64 kernel...\r\n");
        ptr = p + ((Elf64_Ehdr*)p)->e_phoff;
        for(i = 0; !rq && i < ((Elf64_Ehdr*)p)->e_phnum && ptr + ((Elf64_Ehdr*)p)->e_phentsize < kernel_buf + sizeof(kernel_buf);
          i++, ptr += ((Elf64_Ehdr*)p)->e_phentsize)
            if(((Elf64_Phdr*)ptr)->p_type == PT_LOAD && !fw_loadseg(
                (((Elf64_Phdr*)ptr)->p_offset), (((Elf64_Phdr*)ptr)->p_filesz),
                (((Elf64_Phdr*)ptr)->p_vaddr), (((Elf64_Phdr*)ptr)->p_memsz))) goto err;
    } else
    if(((mz_hdr*)p)->magic == MZ_MAGIC && ((pe_hdr*)(p + ((mz_hdr*)p)->peaddr))->magic == PE_MAGIC &&
       ((pe_hdr*)(p + ((mz_hdr*)p)->peaddr))->file_type == PE_OPT_MAGIC_PE32PLUS &&
       ((pe_hdr*)(p + ((mz_hdr*)p)->peaddr))->machine == IMAGE_FILE_MACHINE_ARM64) {
        /* it's a Multiboot2 COFF/PE kernel */
        pe = (pe_hdr*)(p + ((mz_hdr*)p)->peaddr);
        offs = (uint32_t)pe->data.pe64.img_base;
        kernel_mode = MODE_MB64;
        kernel_entry = offs + (uint8_t*)(uintptr_t)pe->entry_point;
        if(verbose) printf("Loading Multiboot2 PE64 kernel...\r\n");
        sec = (pe_sec*)((uint8_t*)pe + pe->opt_hdr_size + 24);
        for(i = 0; !rq && i < pe->sections && (uint8_t*)&sec[1] < kernel_buf + sizeof(kernel_buf); i++, sec++)
            /* the PE section vaddr field is only 32 bits, we must make sure that it properly sign extended to 64 bit */
            if(!fw_loadseg(sec->raddr, sec->rsiz, offs + (int64_t)(int32_t)sec->vaddr, sec->vsiz)) goto err;
    } else
#endif
    {
        printf("ERROR: unknown kernel format\r\n");
err:    fw_close();
        smp = 0;
        return 0;
    }
    fw_close();
    if(kernel_mode != MODE_MB64) smp = 0;
    return 1;
}

/**
 * Finish up MBI tags
 */
void fw_fini(void)
{
    int i, n = 0;
    fadt_t *fadt;
    multiboot_tag_t *t;
    multiboot_tag_smp_t *st = NULL;
    uint8_t *rsdt = NULL, *lapic = NULL, *p, *q, *e, *ptr, *end, s;
    static uint8_t ids[256];
#if CONFIG(LP_ARCH_X86)
    uint32_t a;
#endif
#if CONFIG(LP_ARCH_ARM64)
    register uint64_t reg;
#endif

    if(tags_ptr && vidmode.framebuffer_addr) {
        vidmode.type = MULTIBOOT_TAG_TYPE_FRAMEBUFFER;
        vidmode.size = sizeof(vidmode);
        vidmode.framebuffer_type = 1;
        vidmode.reserved = 0;
        memcpy(tags_ptr, &vidmode, vidmode.size);
        tags_ptr += (vidmode.size + 7) & ~7;
    }
    if(tags_ptr) {
        if(dsdt_ptr && !(
          (((uint8_t*)(uintptr_t)dsdt_ptr)[0] == 0xD0 && ((uint8_t*)(uintptr_t)dsdt_ptr)[1] == 0x0D &&
          ((uint8_t*)(uintptr_t)dsdt_ptr)[2] == 0xFE && ((uint8_t*)(uintptr_t)dsdt_ptr)[3] == 0xED) ||
          (((((uint8_t*)(uintptr_t)dsdt_ptr)[0] == 'D' && ((uint8_t*)(uintptr_t)dsdt_ptr)[1] == 'S') ||
          (((uint8_t*)(uintptr_t)dsdt_ptr)[0] == 'G' && ((uint8_t*)(uintptr_t)dsdt_ptr)[1] == 'U')) &&
          ((uint8_t*)(uintptr_t)dsdt_ptr)[2] == 'D' && ((uint8_t*)(uintptr_t)dsdt_ptr)[3] == 'T'))) dsdt_ptr = 0;
        /* look for the RSD PTR */
        for(t = (multiboot_tag_t*)(tags_buf + sizeof(multiboot_info_t)); (uint8_t*)t < tags_ptr;
          t = (multiboot_tag_t*)((uint8_t*)t + ((t->size + 7) & ~7))) {
            if(t->type == MULTIBOOT_TAG_TYPE_ACPI_OLD || t->type == MULTIBOOT_TAG_TYPE_ACPI_NEW) {
                rsdt = t->type == MULTIBOOT_TAG_TYPE_ACPI_OLD ?
                    (uint8_t*)(uintptr_t)*((uint32_t*)&((multiboot_tag_old_acpi_t*)t)->rsdp[16]) :
                    (uint8_t*)(uintptr_t)*((uint64_t*)&((multiboot_tag_new_acpi_t*)t)->rsdp[24]);
                /* found RSDP, iterate on ACPI tables */
                if((rsdt[0] == 'R' || rsdt[0] == 'X') && !memcmp(rsdt + 1, "SDT", 3))
                    for(ptr = rsdt + 36, end = (uint8_t*)(rsdt + ((fadt_t*)rsdt)->hdr.size); ptr < end;
                      ptr += rsdt[0] == 'X' ? 8 : 4) {
                        p = rsdt[0] == 'X' ? (uint8_t*)((uintptr_t)*((uint64_t*)ptr)) : (uint8_t*)((uintptr_t)*((uint32_t*)ptr));
                        fadt = (fadt_t*)p;
                        /* found FADT, patch DSDT addresses and recalculate checksum */
                        if(dsdt_ptr && !memcmp(&fadt->hdr.magic, "FACP", 4)) {
                            fadt->dsdt = (uint32_t)dsdt_ptr;
                            if(fadt->hdr.rev >= 2 && fadt->hdr.size > sizeof(fadt_t)) fadt->x_dsdt = dsdt_ptr;
                            fadt->hdr.chksum = 0;
                            for(s = 0, i = 0; i < (int)fadt->hdr.size; i++) { s += *(((uint8_t*)fadt) + i); }
                            fadt->hdr.chksum = 0x100 - s;
                        } else
                        if(smp && !memcmp(p, "APIC", 4)) {
                            if(!lapic) lapic = (uint8_t*)(uintptr_t)(*((uint32_t*)(p + 0x24)));
                            for(n = 0, q = p + 44, e = p + *((uint32_t*)(p + 4)); q < e && q[1]; q += q[1])
                                switch(q[0]) {
                                    case 0: if((q[4] & 1) && q[3] != 0xFF) ids[n++] = q[3]; break;
                                    case 5: lapic = (uint8_t*)(uintptr_t)*((uint64_t*)(q + 4)); break;
                                }
                        }
                    }
            }
        }
        /* multicore */
        if(smp) {
            ((multiboot_tag_smp_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_SMP;
            ((multiboot_tag_smp_t*)tags_ptr)->size = sizeof(multiboot_tag_smp_t);
            st = (multiboot_tag_smp_t*)tags_ptr;
            st->numcores = st->running = st->bspid = 0;
#if CONFIG(LP_ARCH_X86)
            st->numcores = n; st->running = 1;
            __asm__ __volatile__ ("movl $1, %%eax; cpuid; shrl $24, %%ebx;" : "=b"(a) : : );
            st->bspid = a; *((uint64_t*)0x8fff8) = a;
#endif
#if CONFIG(LP_ARCH_ARM64)
            ((multiboot_tag_smp_t*)tags_ptr)->numcores = ((multiboot_tag_smp_t*)tags_ptr)->running = n = 4;
#endif
            tags_ptr += (((multiboot_tag_smp_t*)tags_ptr)->size + 7) & ~7;
        }
        /* partition UUIDs */
        ((multiboot_tag_partuuid_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_PARTUUID;
        ((multiboot_tag_partuuid_t*)tags_ptr)->size = 24;
        memcpy(((multiboot_tag_partuuid_t*)tags_ptr)->bootuuid, &bootuuid, sizeof(guid_t));
        tags_ptr += (((multiboot_tag_partuuid_t*)tags_ptr)->size + 7) & ~7;
        /* terminator tag */
        ((multiboot_tag_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_END;
        ((multiboot_tag_t*)tags_ptr)->size = 8;
        tags_ptr += (((multiboot_tag_t*)tags_ptr)->size + 7) & ~7;
        ((multiboot_info_t*)tags_buf)->total_size = tags_ptr - tags_buf;
    } else tags_buf = NULL;

    if(kernel_mode == MODE_LIN && zero_page) {
        if(memmap && num_memmap) {
            for(i = 0; (uint32_t)i < num_memmap && i < E820_MAX_ENTRIES_ZEROPAGE; i++) {
                zero_page->e820_table[i].addr = memmap[i].base_addr;
                zero_page->e820_table[i].size = memmap[i].length;
                zero_page->e820_table[i].type = memmap[i].type;
            }
            zero_page->e820_entries = i;
        }
        if(vidmode.framebuffer_addr) {
            zero_page->lfb_width = vidmode.framebuffer_width;
            zero_page->lfb_height = vidmode.framebuffer_height;
            zero_page->lfb_depth = vidmode.framebuffer_bpp;
            zero_page->lfb_base = vidmode.framebuffer_addr;
            zero_page->lfb_size = (vidmode.framebuffer_pitch * vidmode.framebuffer_height * vidmode.framebuffer_bpp) >> 3;
            zero_page->lfb_linelength = vidmode.framebuffer_pitch;
            zero_page->red_size = vidmode.framebuffer_red_mask_size;
            zero_page->red_pos = vidmode.framebuffer_red_field_position;
            zero_page->green_size = vidmode.framebuffer_green_mask_size;
            zero_page->green_pos = vidmode.framebuffer_green_field_position;
            zero_page->blue_size = vidmode.framebuffer_blue_mask_size;
            zero_page->blue_pos = vidmode.framebuffer_blue_field_position;
            zero_page->orig_video_isVGA = VIDEO_TYPE_VLFB;
            zero_page->hdr.vid_mode = VIDEO_MODE_CUR;
        }
        zero_page->acpi_rsdp_addr = rsdp_ptr;
        if(initrd) {
            zero_page->hdr.ramdisk_image = initrd->mod_start;
            zero_page->hdr.ramdisk_size = initrd->mod_end - initrd->mod_start;
        }
    }
#if CONFIG(LP_ARCH_X86)
    /* new GDT (this must be below 1M because we need it in real mode) */
    __asm__ __volatile__("repnz stosb"::"D"(0x500),"a"(0),"c"(256):);
    *((uint16_t*)0x510) = 0x3F; *((uint32_t*)0x512) = 0x560;            /* value */
    *((uint32_t*)0x568) = 0x0000FFFF; *((uint32_t*)0x56C) = 0x00009800; /*   8 - legacy real cs */
    *((uint32_t*)0x570) = 0x0000FFFF; *((uint32_t*)0x574) = 0x00CF9A00; /*  16 - prot mode cs */
    *((uint32_t*)0x578) = 0x0000FFFF; *((uint32_t*)0x57C) = 0x00CF9200; /*  24 - prot mode ds */
    *((uint32_t*)0x580) = 0x0000FFFF; *((uint32_t*)0x584) = 0x00AF9A00; /*  32 - long mode cs */
    *((uint32_t*)0x588) = 0x0000FFFF; *((uint32_t*)0x58C) = 0x00CF9200; /*  40 - long mode ds */
    *((uint32_t*)0x590) = 0x00000068; *((uint32_t*)0x594) = 0x00008900; /*  48 - long mode tss descriptor */
    *((uint32_t*)0x598) = 0x00000000; *((uint32_t*)0x59C) = 0x00000000; /*       cont. */
    if(smp && n > 1 && lapic && st) {
/* Memory layout (only valid when kernel entry isn't zero)
 *    0x510 -   0x520   GDT value
 *    0x520 -   0x530   IDT value (not used)
 *    0x530 -   0x538   page table root
 *    0x538 -   0x540   kernel entry point (also SMP semaphor)
 *    0x540 -   0x548   tags_buf
 *    0x548 -   0x550   CPU clockcycles in 1 msec
 *    0x550 -   0x558   lapic address
 *    0x558 -   0x559   AP is running flag
 *    0x560 -   0x590   GDT table
 */
        if(verbose) printf("Initializing SMP (%d cores)...\n", n);
        *((volatile uint64_t*)0x530) = (uint64_t)(uintptr_t)pt;
        *((volatile uint64_t*)0x538) = (uint64_t)0;
        *((volatile uint64_t*)0x540) = (uintptr_t)tags_buf;
        *((volatile uint64_t*)0x550) = (uint64_t)(uintptr_t)lapic;
        /* relocate AP startup code to 0x8000 */
        __asm__ __volatile__(
        /* relocate code */
        ".byte 0xe8;.long 0;"
        "1:popl %%esi;addl $1f - 1b, %%esi;movl $0x8000, %%edi;movl $99f - 1f, %%ecx;repnz movsb;jmp 99f;"
        /* do the real mode -> prot mode -> long mode trampoline */
        "1:.code16;cli;cld;xorw %%ax, %%ax;movw %%ax, %%ds;incb (0x558);"
        /* spinlock waiting for the kernel entry address */
        "2:pause;cmpl $0, (0x538);jnz 3f;cmpl $0, (0x53C);jz 2b;3:;"
        /* initialize AP */
        "lgdt (0x510);movl %%cr0, %%eax;orb $1, %%al;movl %%eax, %%cr0;"
        ".code32;ljmp $16,$2f-1b+0x108000;2:;"
        "movw $24, %%ax;movw %%ax, %%ds;"
        "movl (0x530), %%eax;movl %%eax, %%cr3;"
        "movl $0xE0, %%eax;movl %%eax, %%cr4;"
        "movl $0x0C0000080, %%ecx;rdmsr;btsl $8, %%eax;wrmsr;"
        "movl %%cr0, %%eax;xorb %%cl, %%cl;orl %%ecx, %%eax;btcl $16, %%eax;btsl $31, %%eax;movl %%eax, %%cr0;"
        "lgdt (0x510);ljmp $32,$1f-1b+0x8000;"
        ".code64;1:;lgdt (0x510);movw $40, %%ax;movw %%ax, %%ds;movw %%ax, %%es;movw %%ax, %%ss;"
        "xorq %%rax, %%rax;lidt (%%rax);movq (0x550), %%rbx;"
        /* enable lapic */
        "movl $0x1000000, 0xD0(%%rbx);movl $0xFFFFFFFF, 0xE0(%%rbx);"
        "movl 0xF0(%%rbx), %%eax;orl $0x100, %%eax;movl %%eax,0xF0(%%rbx);"
        "movl $0, 0x80(%%rbx);movl $0, 0x280(%%rbx);movl 0x280(%%rbx), %%eax;movl 0x20(%%rbx), %%eax;"
        "shrl $24, %%eax;andq $0xff, %%rax;movq %%rax, %%rbx;shll $10, %%eax;"
        /* execute 64-bit kernel */
        "movq $0x90000, %%rsp;subq %%rax, %%rsp;movq %%rsp, %%rbp;pushq %%rbx;"             /* stack = 0x90000 - coreid * 1024 */
        "movq (0x530), %%rax;movq %%rax, %%cr3;"                                            /* kick the MMU to flush cache */
        "xorq %%rax, %%rax;movl $0x36d76289, %%eax;movq %%rax, %%rcx;movq %%rax, %%rdi;"    /* set arguments */
        "movq (0x540), %%rbx;movq %%rbx, %%rdx;movq %%rbx, %%rsi;"
        /* execute 64-bit kernel (stack: 8 byte aligned, and contains the core's id) */
        /* note: the entry point's function prologue will push rbp, and after that the stack becomes 16 byte aligned as expected */
        "movq (0x538), %%r8;jmp *%%r8;"                                                     /* jump to kernel entry */
        "99:;.code32;":::);

        /* enable Local APIC */
        *((volatile uint32_t*)(lapic + 0x0D0)) = (1 << 24);
        *((volatile uint32_t*)(lapic + 0x0E0)) = 0xFFFFFFFF;
        *((volatile uint32_t*)(lapic + 0x0F0)) = *((volatile uint32_t*)(lapic + 0x0F0)) | 0x1FF;
        *((volatile uint32_t*)(lapic + 0x080)) = 0;
        st->bspid = *((volatile uint32_t*)(lapic + 0x20)) >> 24; *((uint64_t*)0x8fff8) =  st->bspid;
        /* initialize APs */
        for(i = 0; i < n; i++) {
            if(ids[i] == st->bspid) continue;
            *((volatile uint32_t*)(lapic + 0x280)) = 0;                 /* clear APIC errors */
            a = *((volatile uint32_t*)(lapic + 0x280));
            send_ipi(ids[i], 0xfff00000, 0x00C500);                     /* trigger INIT IPI */
            sleep(1);
            send_ipi(ids[i], 0xfff00000, 0x008500);                     /* deassert INIT IPI */
        }
        sleep(10);                                                      /* wait 10 msec */
        /* start APs */
        for(i = 0; i < n; i++) {
            if(ids[i] == st->bspid) continue;
            *((volatile uint8_t*)0x558) = 0;
            send_ipi(ids[i], 0xfff0f800, 0x004608);                     /* trigger SIPI, start at 0800:0000h */
            for(a = 250; !*((volatile uint8_t*)0x558) && a > 0; a--)    /* wait for AP with 250 msec timeout */
                sleep(1);
            if(!*((volatile uint8_t*)0x558)) {
                send_ipi(ids[i], 0xfff0f800, 0x004608);
                sleep(250);
            }
            if(*((volatile uint8_t*)0x558)) st->running++;
        }
    } else smp = 0;
#endif
#if CONFIG(LP_ARCH_ARM64)
    /* enable paging */
    reg=(0xFF << 0) |    /* Attr=0: normal, IWBWA, OWBWA, NTR */
        (0x04 << 8) |    /* Attr=1: device, nGnRE (must be OSH too) */
        (0x44 <<16);     /* Attr=2: non cacheable */
    __asm__ __volatile__("msr mair_el1, %0" ::"r"(reg):);
    *((uint64_t*)0x518) = reg;
    __asm__ __volatile__("mrs %0, id_aa64mmfr0_el1" :"=r"(reg)::);
    reg = (reg & 0xF) << 32L; /* IPS=autodetected */
    reg=(0x00LL << 37) | /* TBI=0, no tagging */
        (0x02LL << 30) | /* TG1=4k */
        (0x03LL << 28) | /* SH1=3 inner */
        (0x01LL << 26) | /* ORGN1=1 write back */
        (0x01LL << 24) | /* IRGN1=1 write back */
        (0x00LL << 23) | /* EPD1 undocumented by ARM DEN0024A Fig 12-5, 12-6 */
        (25LL   << 16) | /* T1SZ=25, 3 levels (512G) */
        (0x00LL << 14) | /* TG0=4k */
        (0x03LL << 12) | /* SH0=3 inner */
        (0x01LL << 10) | /* ORGN0=1 write back */
        (0x01LL << 8) |  /* IRGN0=1 write back */
        (0x00LL << 7) |  /* EPD0 undocumented by ARM DEN0024A Fig 12-5, 12-6 */
        (25LL   << 0);   /* T0SZ=25, 3 levels (512G) */
    __asm__ __volatile__("msr tcr_el1, %0; isb" ::"r" (reg):);
    *((uint64_t*)0x520) = reg;
    __asm__ __volatile__("msr ttbr0_el1, %0" ::"r"((uintptr_t)pt + 1):);
    *((uint64_t*)0x528) = (uintptr_t)pt + 1;
    __asm__ __volatile__("msr ttbr1_el1, %0" ::"r"((uintptr_t)pt + 1 + 4096):);
    *((uint64_t*)0x530) = (uintptr_t)pt + 1 + 4096;
    /* set mandatory reserved bits */
    __asm__ __volatile__("dsb ish; isb; mrs %0, sctlr_el1" :"=r"(reg)::);
    reg |= 0xC00801;
    reg&=~( (1<<25) |   /* clear EE, little endian translation tables */
            (1<<24) |   /* clear E0E */
            (1<<19) |   /* clear WXN */
            (1<<12) |   /* clear I, no instruction cache */
            (1<<4) |    /* clear SA0 */
            (1<<3) |    /* clear SA */
            (1<<2) |    /* clear C, no cache at all */
            (1<<1));    /* clear A, no aligment check */
    __asm__ __volatile__("msr sctlr_el1, %0; isb" ::"r"(reg):);
    *((uint64_t*)0x508) = reg;
    if(smp) {
        if(verbose) printf("Initializing SMP (%d cores)...\n", n);
        *((uint64_t*)0x540) = (uintptr_t)tags_buf;
        *((uint64_t*)0x538) = 0;
/* Memory layout (only valid when kernel entry isn't zero)
 *    0x508 -   0x510   sctlr
 *    0x510 -   0x518   vbar
 *    0x518 -   0x520   mair
 *    0x520 -   0x528   tcr
 *    0x528 -   0x530   ttbr0
 *    0x530 -   0x538   ttbr1
 *    0x538 -   0x540   kernel entry point (also SMP semaphor)
 *    0x540 -   0x548   tags_buf
 */
        __asm__ __volatile__(
        "mov w2, %w0; mov x1, x30; bl 1f;1:mov x0, x30;mov x30, x1;add x0, x0, #2f-1b;"
        "mov x1, #0xE0; 9:str x0, [x1], #0;add x1, x1, #8;sub w2, w2, #1;cbnz w2, 9b; b 99f;"
        "2:mov x1, #0x1000;"
        "mrs x0, CurrentEL;and x0, x0, #12;"
        "cmp x0, #12;bne 1f;"                           /* are we running at EL3? */
        "mov x0, #0x5b1;msr scr_el3, x0;mov x0, #0x3c9;msr spsr_el3, x0;adr x0, 1f;msr elr_el3, x0;mov x0, #4;msr sp_el2, x1;eret;"
        "1:cmp x0, #4;beq 1f;"                          /* are we running at EL2? */
        "mrs x0,cnthctl_el2;orr x0,x0,#3;msr cnthctl_el2,x0;msr cnthp_ctl_el2,xzr;"         /* enable CNTP */
        "mov x0,#(1 << 31);orr x0,x0,#2;msr hcr_el2,x0;mrs x0,hcr_el2;"                     /* enable Aarch64 at EL1 */
        "mrs x0,midr_el1;mrs x2,mpidr_el1;msr vpidr_el2,x0;msr vmpidr_el2,x2;"              /* initialize virtual MPIDR */
        "mov x0,#0x33FF;msr cptr_el2,x0;msr hstr_el2,xzr;mov x0,#(3<<20);msr cpacr_el1,x0;" /* disable coprocessor traps */
        "mov x2,#0x0800;movk x2,#0x30d0,lsl #16;msr sctlr_el1, x2;"                         /* setup SCTLR access */
        "mov x2,#0x3c5;msr spsr_el2,x2;adr x2, 1f;msr elr_el2, x2;mov sp, x1;msr sp_el1, x1;eret;"/* switch to EL1 */
        "1:mov sp, x1;mov x2, #0x500;ldr x0, [x2], #0x10;msr vbar_el1,x0;msr SPSel,#0;"     /* set up exception handlers */
        /* spinlock waiting for the kernel entry address */
        "1:ldr x30, [x2], #0x38;nop;nop;nop;nop;cbz x30, 1b;"
        /* initialize AP */
        "ldr x0, [x2], #0x18;msr mair_el1,x0;"
        "ldr x0, [x2], #0x20;msr tcr_el1,x0;"
        "ldr x0, [x2], #0x28;msr ttbr0_el1,x0;"
        "ldr x0, [x2], #0x30;msr ttbr1_el1,x0;"
        "ldr x0, [x2], #0x08;dsb ish;msr sctlr_el1,x0;isb;"
        /* execute 64-bit kernel (stack: 16 byte aligned, and contains the core's id) */
        "mov sp, #0x80000;mrs x0, mpidr_el1;and x0, x0, #3;lsl x1,x0,#10;sub sp,sp,x1;"     /* stack = 0x80000 - coreid * 1024 */
        "str x0, [sp, #-16]!;ldr x0, =0x36d76289;ldr x1, [x2], #0x40;ret;"                  /* jump to kernel entry */
        "99:"::"r"(n - 1):);
    }
#endif
    *((uint64_t*)0x558) = 0;
}

/*****************************************
 *     Simpleboot loader entry point     *
 *****************************************/
int main(void)
{
    fw_init();
    printf("Simpleboot loader, Copyright (c) 2023 bzt, MIT license\r\n");
    /* now that we can display error messages, let's see if we got everything we need */
    if(!pt) { printf("ERROR: unable to allocate memory\r\n"); goto err; }
    if(!root_dir) { printf("ERROR: unable to locate boot partition\r\n"); }

    /* load and parse simpleboot.cfg */
again:
    fw_loadconfig();
    fw_loadsetup();
    if(ram < 64) { printf("ERROR: unable to determine the amount of RAM\r\n"); goto err; }
    else if(verbose && !bkp) printf("Physical RAM %llu Megabytes\r\n", ram / 1024 / 1024 + 2);

    /* now we have the kernel's filename, try to load that, it's a critical error if fails */
    if(!fw_loadkernel()) goto err;
    /* last step, load modules too */
    fw_loadmodules();
    /* if the user pressed a key during loading, fallback to backup and do over */
    if(!bkp && rq) { bkp++; rq = 0; goto again; }

    /* finish up things, finalize tags list */
    fw_fini();

    /* transfer control to kernel. Should never return */
    if(!kernel_entry) { printf("ERROR: no kernel entry point\r\n"); goto err; }
    if(verbose > 2) { printf("Kernel entry:\r\n"); hexdump(kernel_entry, 4); }

    switch(kernel_mode) {
#if CONFIG(LP_ARCH_X86)
        case MODE_MB32:
            if(verbose > 1)
                printf("Transfering prot mode control to %08lx(%08x, %08lx[%lx])\r\n", (uintptr_t)kernel_entry,
                    MULTIBOOT2_BOOTLOADER_MAGIC, (uintptr_t)tags_buf, (uintptr_t)(tags_ptr - tags_buf));
            /* execute 32-bit kernels in protected mode */
            *((uint32_t*)0x8fffc) = (uint32_t)(uintptr_t)tags_buf;
            *((uint32_t*)0x8fff8) = MULTIBOOT2_BOOTLOADER_MAGIC;
            *((uint32_t*)0x8fff4) = 0xDEADBEEF;
            __asm__ __volatile__(
            "xorl %%eax, %%eax;lidt (%%eax);"           /* disable IDT */
            /* CDECL uses the stack for arguments, but fastcall uses %ecx, %edx */
            "movl $0x8fff4, %%esp; movl %%esp, %%ebp;"
            "movl 8(%%esp), %%edx;movl %%edx, %%ebx;movl 4(%%esp), %%eax; movl %%eax, %%ecx;"
            "cli;cld;jmp *%%esi;"
            ::"S"(kernel_entry):);
        break;
        case MODE_MB64:
            if(verbose > 1)
                printf("Transfering long mode control to %08lx(%08x, %08lx[%lx])\r\n", (uintptr_t)kernel_entry,
                    MULTIBOOT2_BOOTLOADER_MAGIC, (uintptr_t)tags_buf, (uintptr_t)(tags_ptr - tags_buf));
            /* tell APs to execute kernel */
            if(smp) { *((volatile uint64_t*)0x538) = (uintptr_t)kernel_entry; __asm__ __volatile__("pause":::"memory"); }
            /* execute 64-bit kernels in long mode */
            __asm__ __volatile__(
            "movl $0x1000, %%eax; movl %%eax, %%cr3;"   /* page tables */
            "movl $0x00E0, %%eax; movl %%eax, %%cr4;"   /* set PAE, MCE, PGE; clear everything else */
            "movl $0xC0000080, %%ecx;"
            "rdmsr; btsl $8, %%eax; wrmsr;"             /* EFER MSR */
            "movl $0xC0000011, %%eax;movl %%eax, %%cr0;"/* clear EM, MP, WP, enable paging with cache disabled (set PE, CD) */
            "lgdt (0x510);ljmp $32, $1f;.code64;1:;"    /* set segments */
            "movw $40, %%ax;movw %%ax, %%ds;movw %%ax, %%es;movw %%ax, %%ss;"
            "xorq %%rax, %%rax;lidt (%%rax);"           /* disable IDT */
            "movq $0x90000, %%rsp; movq %%rsp, %%rbp; subl $8, %%esp;"
            "movq %%rbx, %%r8;"
            "movq %%rdi, %%rcx; movq %%rdi, %%rax;"
            "movq %%rsi, %%rdx; movq %%rsi, %%rbx;"
            "cli;cld;jmp *%%r8;.code32;"
            ::"D"(MULTIBOOT2_BOOTLOADER_MAGIC),"S"(tags_buf),"b"(kernel_entry):);
        break;
        case MODE_LIN:
            if(verbose > 1)
                printf("Transfering long mode control to %08lx(%08lx)\r\n", (uintptr_t)kernel_entry, (uintptr_t)zero_page);
            /* execute Linux kernel in 64 bit mode */
            __asm__ __volatile__(
            "movl $0x1000, %%eax; movl %%eax, %%cr3;"   /* page tables */
            "movl $0x00E0, %%eax; movl %%eax, %%cr4;"   /* set PAE, MCE, PGE; clear everything else */
            "movl $0xC0000080, %%ecx;"
            "rdmsr; btsl $8, %%eax; wrmsr;"             /* EFER MSR */
            "movl $0xC0000011, %%eax;movl %%eax, %%cr0;"/* clear EM, MP, WP, enable paging with cache disabled (set PE, CD) */
            "lgdt (0x510);ljmp $32, $1f;.code64;1:;"    /* set segments */
            "movw $40, %%ax;movw %%ax, %%ds;movw %%ax, %%es;movw %%ax, %%ss;"
            "xorq %%rax, %%rax;lidt (%%rax);"           /* disable IDT */
            "movq $0x90000, %%rsp; movq %%rsp, %%rbp;"
            "cli;cld;jmp *%%rbx;.code32;"
            ::"S"(zero_page),"b"(kernel_entry):);
        break;
#endif
#if CONFIG(LP_ARCH_ARM64)
        case MODE_MB64:
            if(verbose > 1)
                printf("Transfering control to %08x(%08x, %08x[%x])\r\n", kernel_entry,
                    MULTIBOOT2_BOOTLOADER_MAGIC, tags_buf, tags_ptr - tags_buf);
            if(smp) {
                /* tell APs to execute kernel */
                *((volatile uint64_t*)0x538) = (uintptr_t)kernel_entry; __asm__ __volatile__("dsb ish":::"memory");
                /* execute 64-bit kernels in long mode */
                __asm__ __volatile__(
                "ldr x0, =tags_buf; ldr x1, [x0], #0;"  /* MBI tags pointer */
                "ldr x0, =0x36d76289;"                  /* magic */
                "mov sp, #0x80000;str xzr, [sp, #-16]!;"/* stack, bsp id on top */
                "mov x30, %0; ret"                      /* jump to entry point */
                ::"r"(kernel_entry):);
            } else {
                __asm__ __volatile__(
                "ldr x0, =tags_buf; ldr x1, [x0], #0;"  /* MBI tags pointer */
                "ldr x0, =0x36d76289;"                  /* magic */
                "mov sp, #0x80000; mov x30, %0; ret"    /* stack and jump to entry point */
                ::"r"(kernel_entry):);
            }
        break;
        case MODE_LIN:
            if(verbose > 1)
                printf("Transfering control to 80000\r\n");
            __asm__ __volatile__(
            "mov x0, %0; mov x1, xzr; mov x2, xzr; mov x3, xzr;"
            "mov x30,#0x80000; mov sp, x30; ret"
            ::"r"(dsdt_ptr):);
        break;
#endif
    }
    printf("ERROR: kernel should not have returned\r\n");

    /* there's nowhere to return to, halt machine */
err:if(!bkp) { getchar(); bkp++; goto again; }
    halt();
    return 0;
}
