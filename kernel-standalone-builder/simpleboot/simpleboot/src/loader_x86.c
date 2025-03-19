/*
 * src/loader_x86.c
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
 * @brief The main Simpleboot loader program on x86_64
 *
 * Memory layout when booted on UEFI:
 *     Arbitrary, uses relocations. Data buffers are allocated through BootServices
 *    0x510 -   0x5A0   AP startup data
 *   0x8000 -  0x80FF   relocated AP startup code
 *
 * Memory layout when booted on BIOS:
 *      0x0 -   0x400   IVT
 *    0x400 -   0x4FF   BDA
 *    0x4FF -   0x500   BIOS boot drive code
 *    0x500 -   0x510   BIOS LBA packet
 *    0x510 -   0x520   GDT value
 *    0x520 -   0x530   IDT value
 *    0x580 -   0x600   EDID info
 *    0x600 -   0x800   temporary disk, E820 or VESA buffer
 *    0x800 -   0xB00   temporary VESA buffer continued
 *    0xB00 -  0x1000   stack (ca. 1280 bytes)
 *   0x1000 -  0x8000   paging tables
 *   0x8000 - 0x20000   our COFF sections (0x8000 - 0x80FF relocated AP startup code)
 *  0x20000 - 0x90000   config + logo + tags; from the top to bottom: kernel's stack
 *  0x90000 - 0x9A000   Linux kernel only: zero page + cmdline
 *  0x9A000 - 0xA0000   EBDA
 *  0xA0000 - 0xFFFFF   VRAM and BIOS ROM
 * 0x100000 -      x    kernel segments, followed by the modules, each page aligned
 */

/**
 * The longest path we can handle
 */
#define PATH_MAX 1024
/**
 * Maximum size of MBI tags in pages (30 * 4096 = 122k)
 */
#define TAGS_MAX 30

/**
 * Specify where the boot messages should appear, make it a comment to disable
 */
#define CONSOLE_SERIAL  0x3f8                       /* default serial, IO base address of COM1 port */
#define CONSOLE_FB                                  /* on screen too */
#define CONSOLE_VGA                                 /* fallback text mode console in BIOS */
/*#define CONSOLE_BOCHS_E9*/                        /* bochs E9 port hack */
/*#define CONSOLE_UEFI*/                            /* UEFI ConOut */

/* it is VERY important that these two variables must be the first in the
 * read-only data segment, because the disk generator might alter them */
const char defkernel[64] = "kernel", definitrd[64] = "";

#include "../simpleboot.h"
#include "loader.h"

/* IMPORTANT: don't assume .bss is zeroed out like in a hosted environment, because it's not */
efi_system_table_t *ST;
esp_bpb_t *bpb;
fossbios_t *FB;
multiboot_mmap_entry_t *memmap;
multiboot_tag_module_t *initrd;
multiboot_tag_framebuffer_t vidmode;
uint64_t vbr_lba, vbr_size, file_size, rsdp_ptr, dsdt_ptr, file_buf, mod_buf, ram, *pt, hack_buf;
uint32_t fb_w, fb_h, fb_bpp, fb_bg, logo_size, verbose, num_memmap, pb_b, pb_m, pb_l, rq, bkp, smp;
uint16_t bootdev, wcname[PATH_MAX];
uint8_t *tags_buf, *tags_ptr, *logo_buf, *kernel_entry, kernel_mode, kernel_buf[4096], *pb_fb, in_exc;
char *conf_buf, *kernel, *cmdline;
const char excstr[32][3] = { "DE", "DB", "NI", "BP", "OF", "BR", "UD", "DF", "CO", "TS", "NP", "SS", "GP", "PF",
    "15", "MF", "AC", "MC", "XF", "20", "CP", "22", "23", "24", "25", "26", "27", "HV", "VC", "SX", "31" };
linux_boot_params_t *zero_page;
rsdt_t __attribute__((aligned(16))) rsdt;
fadt_t __attribute__((aligned(16))) fadt;

#define sleep(n) do { \
        __asm__ __volatile__ ( "rdtsc" : "=a"(a),"=d"(b)); d = (((uint64_t)b << 32UL)|(uint64_t)a) + n*(*((uint64_t*)0x548)); \
            do { __asm__ __volatile__ ( "pause;rdtsc" : "=a"(a),"=d"(b)); c = ((uint64_t)b << 32UL)|(uint64_t)a; } while(c < d); \
    } while(0)
#define send_ipi(a,m,v) do { \
        while(*((volatile uint32_t*)(lapic + 0x300)) & (1 << 12)) __asm__ __volatile__ ("pause" : : : "memory"); \
        *((volatile uint32_t*)(lapic + 0x310)) = (*((volatile uint32_t*)(lapic + 0x310)) & 0x00ffffff) | (a << 24); \
        *((volatile uint32_t*)(lapic + 0x300)) = (*((volatile uint32_t*)(lapic + 0x300)) & m) | v;  \
    } while(0)

/**************** These will be overriden by the SMP trampoline code ****************/

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

/**************** Mandatory functions, Clang generates calls to them ****************/

void memcpy(void *dst, const void *src, uint32_t n) { __asm__ __volatile__("repnz movsb"::"D"(dst),"S"(src),"c"(n):); }
void memset(void *dst, uint8_t c, uint32_t n) { __asm__ __volatile__("repnz stosb"::"D"(dst),"a"(c),"c"(n):); }
int  memcmp(const void *s1, const void *s2, uint32_t n) {
    int ret;
    __asm__ __volatile__("cld;repe cmpsb;xorl %%eax,%%eax;movb -1(%%rdi), %%al;subb -1(%%rsi), %%al;"
    :"=a"(ret)
    :"D"(s1),"S"(s2),"c"(n):);
    return ret;
}

/**************** Early boot console ****************/

#ifdef CONSOLE_FB
typedef struct { uint32_t magic, version, headersize, flags, numglyph, bytesperglyph, height, width; } __attribute__((packed)) psf2_t;
uint8_t font_psf[2080] = { 114,181,74,134,0,0,0,0,32,0,0,0,0,0,12,0,128,0,0,0,16,0,0,0,16,0,0,0,8,0,0,0,0,0,218,2,128,130,2,128,130,2,128,182,0,0,0,0,0,0,126,129,165,129,129,189,153,129,129,126,0,0,0,0,0,0,126,255,219,255,255,195,231,255,255,126,0,0,0,0,0,0,0,0,108,254,254,254,254,124,56,16,0,0,0,0,0,0,0,0,16,56,124,254,124,56,16,0,0,0,0,0,0,0,0,24,60,60,231,231,231,24,24,60,0,0,0,0,0,0,0,24,60,126,255,255,126,24,24,60,0,0,0,0,0,0,0,0,0,0,24,60,60,24,0,0,0,0,0,0,255,255,255,255,255,255,231,195,195,231,255,255,255,255,255,255,0,0,0,0,0,60,102,66,66,102,60,0,0,0,0,0,255,255,255,255,255,195,153,189,189,153,195,255,255,255,255,255,0,0,30,14,26,50,120,204,204,204,204,120,0,0,0,0,0,0,60,102,102,102,102,60,24,126,24,24,0,0,0,0,0,0,63,51,63,48,48,48,48,112,240,224,0,0,0,0,0,0,127,99,127,99,99,99,99,103,231,230,192,0,0,0,0,0,0,24,24,219,60,231,60,219,24,24,0,0,0,0,0,128,192,224,240,248,254,248,240,224,192,128,0,0,0,0,0,2,6,14,30,62,254,62,30,14,6,2,0,0,0,0,0,0,24,60,126,24,24,24,126,60,24,0,0,0,0,0,0,0,102,102,102,102,102,102,102,0,102,102,0,0,0,0,0,0,127,219,219,219,123,27,27,27,27,27,0,0,0,0,0,124,198,96,56,108,198,198,108,56,12,198,124,0,0,0,0,0,0,0,0,0,0,0,254,254,254,254,0,0,0,0,0,0,24,60,126,24,24,24,126,60,24,126,0,0,0,0,0,0,24,60,126,24,24,24,24,24,24,24,0,0,0,0,0,0,24,24,24,24,24,24,24,126,60,24,0,0,0,0,0,0,0,0,0,24,12,254,12,24,0,0,0,0,0,0,0,0,0,0,0,48,96,254,96,48,0,0,0,0,0,0,0,0,0,0,0,0,192,192,192,254,0,0,0,0,0,0,0,0,0,0,0,40,108,254,108,40,0,0,0,0,0,0,0,0,0,0,16,56,56,124,124,254,254,0,0,0,0,0,0,0,0,0,254,254,124,124,56,56,16,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,24,60,60,60,24,24,24,0,24,24,0,0,0,0,0,102,102,102,36,0,0,0,0,0,0,0,0,0,0,0,0,0,0,108,108,254,108,108,108,254,108,108,0,0,0,0,24,24,124,198,194,192,124,6,6,134,198,124,24,24,0,0,0,0,0,0,194,198,12,24,48,96,198,134,0,0,0,0,0,0,56,108,108,56,118,220,204,204,204,118,0,0,0,0,0,48,48,48,32,0,0,0,0,0,0,0,0,0,0,0,0,0,12,24,48,48,48,48,48,48,24,12,0,0,0,0,0,0,48,24,12,12,12,12,12,12,24,48,0,0,0,0,0,0,0,0,0,102,60,255,60,102,0,0,0,0,0,0,0,0,0,0,0,24,24,126,24,24,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,24,24,24,48,0,0,0,0,0,0,0,0,0,0,254,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,24,24,0,0,0,0,0,0,0,0,2,6,12,24,48,96,192,128,0,0,0,0,0,0,56,108,198,198,214,214,198,198,108,56,0,0,0,0,0,0,24,56,120,24,24,24,24,24,24,126,0,0,0,0,0,0,124,198,6,12,24,48,96,192,198,254,0,0,0,0,0,0,124,198,6,6,60,6,6,6,198,124,0,0,0,0,0,0,12,28,60,108,204,254,12,12,12,30,0,0,0,0,0,0,254,192,192,192,252,6,6,6,198,124,0,0,0,0,0,0,56,96,192,192,252,198,198,198,198,124,0,0,0,0,0,0,254,198,6,6,12,24,48,48,48,48,0,0,0,0,0,0,124,198,198,198,124,198,198,198,198,124,0,0,0,0,0,0,124,198,198,198,126,6,6,6,12,120,0,0,0,0,0,0,0,0,24,24,0,0,0,24,24,0,0,0,0,0,0,0,0,0,24,24,0,0,0,24,24,48,0,0,0,0,0,0,0,6,12,24,48,96,48,24,12,6,0,0,0,0,0,0,0,0,0,126,0,0,126,0,0,0,0,0,0,0,0,0,0,96,48,24,12,6,12,24,48,96,0,0,0,0,0,0,124,198,198,12,24,24,24,0,24,24,0,0,0,0,0,0,0,124,198,198,222,222,222,220,192,124,0,0,0,0,0,0,16,56,108,198,198,254,198,198,198,198,0,0,0,0,0,0,252,102,102,102,124,102,102,102,102,252,0,0,0,0,0,0,60,102,194,192,192,192,192,194,102,60,0,0,0,0,0,0,248,108,102,102,102,102,102,102,108,248,0,0,0,0,0,0,254,102,98,104,120,104,96,98,102,254,0,0,0,0,0,0,254,102,98,104,120,104,96,96,96,240,0,0,0,0,0,0,60,102,194,192,192,222,198,198,102,58,0,0,0,0,0,0,198,198,198,198,254,198,198,198,198,198,0,0,0,0,0,0,60,24,24,24,24,24,24,24,24,60,0,0,0,0,0,0,30,12,12,12,12,12,204,204,204,120,0,0,0,0,0,0,230,102,102,108,120,120,108,102,102,230,0,0,0,0,0,0,240,96,96,96,96,96,96,98,102,254,0,0,0,0,0,0,198,238,254,254,214,198,198,198,198,198,0,0,0,0,0,0,198,230,246,254,222,206,198,198,198,198,0,0,0,0,0,0,124,198,198,198,198,198,198,198,198,124,0,0,0,0,0,0,252,102,102,102,124,96,96,96,96,240,0,0,0,0,0,0,124,198,198,198,198,198,198,214,222,124,12,14,0,0,0,0,252,102,102,102,124,108,102,102,102,230,0,0,0,0,0,0,124,198,198,96,56,12,6,198,198,124,0,0,0,0,0,0,126,126,90,24,24,24,24,24,24,60,0,0,0,0,0,0,198,198,198,198,198,198,198,198,198,124,0,0,0,0,0,0,198,198,198,198,198,198,198,108,56,16,0,0,0,0,0,0,198,198,198,198,214,214,214,254,238,108,0,0,0,0,0,0,198,198,108,124,56,56,124,108,198,198,0,0,0,0,0,0,102,102,102,102,60,24,24,24,24,60,0,0,0,0,0,0,254,198,134,12,24,48,96,194,198,254,0,0,0,0,0,0,60,48,48,48,48,48,48,48,48,60,0,0,0,0,0,0,0,128,192,224,112,56,28,14,6,2,0,0,0,0,0,0,60,12,12,12,12,12,12,12,12,60,0,0,0,0,16,56,108,198,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,255,0,0,48,48,24,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,120,12,124,204,204,204,118,0,0,0,0,0,0,224,96,96,120,108,102,102,102,102,124,0,0,0,0,0,0,0,0,0,124,198,192,192,192,198,124,0,0,0,0,0,0,28,12,12,60,108,204,204,204,204,118,0,0,0,0,0,0,0,0,0,124,198,254,192,192,198,124,0,0,0,0,0,0,56,108,100,96,240,96,96,96,96,240,0,0,0,0,0,0,0,0,0,118,204,204,204,204,204,124,12,204,120,0,0,0,224,96,96,108,118,102,102,102,102,230,0,0,0,0,0,0,24,24,0,56,24,24,24,24,24,60,0,0,0,0,0,0,6,6,0,14,6,6,6,6,6,6,102,102,60,0,0,0,224,96,96,102,108,120,120,108,102,230,0,0,0,0,0,0,56,24,24,24,24,24,24,24,24,60,0,0,0,0,0,0,0,0,0,236,254,214,214,214,214,198,0,0,0,0,0,0,0,0,0,220,102,102,102,102,102,102,0,0,0,0,0,0,0,0,0,124,198,198,198,198,198,124,0,0,0,0,0,0,0,0,0,220,102,102,102,102,102,124,96,96,240,0,0,0,0,0,0,118,204,204,204,204,204,124,12,12,30,0,0,0,0,0,0,220,118,102,96,96,96,240,0,0,0,0,0,0,0,0,0,124,198,96,56,12,198,124,0,0,0,0,0,0,16,48,48,252,48,48,48,48,54,28,0,0,0,0,0,0,0,0,0,204,204,204,204,204,204,118,0,0,0,0,0,0,0,0,0,102,102,102,102,102,60,24,0,0,0,0,0,0,0,0,0,198,198,214,214,214,254,108,0,0,0,0,0,0,0,0,0,198,108,56,56,56,108,198,0,0,0,0,0,0,0,0,0,198,198,198,198,198,198,126,6,12,248,0,0,0,0,0,0,254,204,24,48,96,198,254,0,0,0,0,0,0,14,24,24,24,112,24,24,24,24,14,0,0,0,0,0,0,24,24,24,24,24,24,24,24,24,24,24,24,0,0,0,0,112,24,24,24,14,24,24,24,24,112,0,0,0,0,0,0,118,220,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,16,56,108,198,198,198,254,0,0,0,0,0 };
uint32_t fb_x, fb_y;
#endif
#ifdef CONSOLE_VGA
uint16_t vga_x, vga_y;
#endif

/**
 * Initialize the console
 */
void console_init(void)
{
#ifdef CONSOLE_SERIAL
    if(FB && FB->serial) FB->serial->setmode(0, 115200, 8, 0, 1); else
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
    : : "a"(CONSOLE_SERIAL + 1): "rdx");
#endif
#ifdef CONSOLE_FB
    fb_x = fb_y = 4;
#endif
#ifdef CONSOLE_VGA
    vga_x = vga_y = 0;
    if(!vidmode.framebuffer_addr && !ST) memset((void*)0xB8000, 0, 160 * 25);
#endif
#ifdef CONSOLE_UEFI
    if(ST && ST->ConOut) ST->ConOut->Reset(ST->ConOut, 0);
#endif
}

/**
 * Display a character on boot console
 */
void console_putc(uint8_t c)
{
#ifdef CONSOLE_UEFI
    uint16_t tmp[2];
#endif
#ifdef CONSOLE_FB
    psf2_t *font = (psf2_t*)font_psf;
    uint32_t x, y, line, mask, offs, bpl = (font->width + 7) >> 3;
    uint8_t *glyph, *fb = (uint8_t*)vidmode.framebuffer_addr;
#endif
    if(FB && FB->serial) FB->serial->send(0, c); else
    __asm__ __volatile__(
        "xorl %%ebx, %%ebx; movb %0, %%bl;"
#ifdef CONSOLE_SERIAL
        "movl $10000,%%ecx;"
        "1:inb %%dx, %%al;pause;"
        "cmpb $0xff,%%al;je 2f;"
        "dec %%ecx;jz 2f;"
        "andb $0x20,%%al;jz 1b;"
        "subb $5,%%dl;movb %%bl, %%al;outb %%al, %%dx;2:;"
#endif
#ifdef CONSOLE_BOCHS_E9
        "movb %%bl, %%al;outb %%al, $0xe9;"
#endif
    ::"a"(c),"d"(CONSOLE_SERIAL + 5): "rbx", "rcx");

#ifdef CONSOLE_FB
    if(fb)
        switch(c) {
            case '\r': fb_x = 4; break;
            case '\n': fb_x = 4; fb_y += font->height; break;
            default:
                if(fb_x + font->width + 5 >= vidmode.framebuffer_width) { fb_x = 4; fb_y += font->height; }
                if(fb_y + font->height + 5 > vidmode.framebuffer_height) {
                    x = fb_y; fb_y = vidmode.framebuffer_height - font->height - 5; x -= fb_y;
                    offs = 0; line = x * vidmode.framebuffer_pitch;
                    for(y = x; y < vidmode.framebuffer_height; y++,
                      offs += vidmode.framebuffer_pitch, line += vidmode.framebuffer_pitch)
                        memcpy(fb + offs, fb + line, vidmode.framebuffer_pitch);
                    for(y = fb_y, offs = fb_y * vidmode.framebuffer_pitch; y < vidmode.framebuffer_height; y++,
                      offs += vidmode.framebuffer_pitch)
                        memset(fb + offs, 0, vidmode.framebuffer_pitch);
                }
                glyph = font_psf + font->headersize + (c > 0 && c < font->numglyph ? c : 0) * font->bytesperglyph;
                offs = fb_y * vidmode.framebuffer_pitch + fb_x * ((vidmode.framebuffer_bpp + 7) >> 3); fb_x += (font->width + 1);
                for(y = 0; y < font->height; y++, glyph += bpl, offs += vidmode.framebuffer_pitch) {
                    line = offs; mask = 1 << (font->width - 1);
                    for(x = 0; x < font->width && mask; x++, mask >>= 1) {
                        switch(vidmode.framebuffer_bpp) {
                            case 15: case 16: *((uint16_t*)(fb + line)) = ((int)*glyph) & mask ? 0xFFFF : fb_bg; line += 2; break;
                            case 24: *((uint32_t*)(fb + line)) = ((int)*glyph) & mask ? 0xFFFFFF : fb_bg; line += 3; break;
                            case 32: *((uint32_t*)(fb + line)) = ((int)*glyph) & mask ? 0xFFFFFFFF : fb_bg; line += 4; break;
                        }
                    }
                    *((uint32_t*)(fb + line)) = fb_bg;
                }
            break;
        }
#endif
#ifdef CONSOLE_VGA
    if(!fb && !ST)
        switch(c) {
            case '\r': vga_x = 0; break;
            case '\n': vga_x = 0; vga_y++; break;
            default:
                if(vga_y >= 25) {
                    memcpy((void*)0xB8000, (void*)0xB8000 + 160, 160 * 24); vga_x = 0; vga_y = 24;
                    memset((void*)0xB8000 + 24 * 160, 0, 160);
                }
                *((uint16_t*)((uintptr_t)0xB8000 + vga_y * 160 + vga_x++ * 2)) = 0x0f00 | (c & 0xff);
            break;
        }
#endif
#ifdef CONSOLE_UEFI
    if(ST && ST->ConOut) { tmp[0] = c; tmp[1] = 0; ST->ConOut->OutputString(ST->ConOut, tmp); }
#endif
}

/**
 * Display (extremely minimal) formated message on console
 * %c: an ASCII character
 * %d: a decimal number
 * %x: a hexadecimal number
 * %p: a pointer
 * %s: a zero terminated ASCII string (8 bit)
 * %S: a zero terminated WCHAR string (16 bit characters, truncated to 8 bit)
 * %D: dump 16 bytes from given address
 */
void printf(char *fmt, ...)
{
    __builtin_va_list args;
    uint8_t *ptr;
    int64_t arg;
    uint16_t *u;
    int len, sign, i, l;
    char *p, tmpstr[19], n;

    __builtin_va_start(args, fmt);
    arg = 0;
    while(*fmt) {
        if(*fmt == '%') {
            fmt++;
            if(*fmt == '%') goto put;
            len=l=0; while(*fmt >= '0' && *fmt <= '9') { len *= 10; len += *fmt - '0'; fmt++; }
            if(*fmt == 'l') { l++; fmt++; }
            if(*fmt == 'c') { arg = __builtin_va_arg(args, int); console_putc((uint8_t)arg); fmt++; continue; } else
            if(*fmt == 'd') {
                if(!l) arg = (int32_t)__builtin_va_arg(args, int32_t);
                else arg = __builtin_va_arg(args, int64_t);
                sign = 0; if((int)arg < 0) { arg = -arg; sign++; }
                i = 18; tmpstr[i] = 0;
                do { tmpstr[--i] = '0' + (arg % 10); arg /= 10; } while(arg != 0 && i > 0);
                if(sign) tmpstr[--i] = '-';
                if(len > 0 && len < 18) { while(i > 18 - len) tmpstr[--i] = ' '; }
                p = &tmpstr[i];
                goto putstring;
            } else
            if(*fmt == 'x' || *fmt == 'p') {
                if(*fmt == 'x' && !l) arg = (int32_t)__builtin_va_arg(args, int32_t);
                else arg = __builtin_va_arg(args, int64_t);
                i = 16; tmpstr[i] = 0; if(*fmt == 'p') len = 16;
                do { n = arg & 0xf; tmpstr[--i] = n + (n > 9 ? 0x37 : 0x30); arg >>= 4; } while(arg != 0 && i > 0);
                if(len > 0 && len <= 16) { while(i > 16 - len) tmpstr[--i] = '0'; }
                p = &tmpstr[i];
                goto putstring;
            } else
            if(*fmt == 's') {
                p = __builtin_va_arg(args, char*);
putstring:      if(p == (void*)0) p = "(null)";
                while(*p) console_putc(*p++);
            }
            if(*fmt == 'S') {
                u = __builtin_va_arg(args, uint16_t*);
                if(u == (void*)0) u = L"(null)";
                while(*u) console_putc(*u++);
            } else
            if(*fmt == 'D') {
                arg = __builtin_va_arg(args, int64_t);
                if(len < 1) len = 1;
                do {
                    for(i = 28; i >= 0; i -= 4) { n = (arg >> i) & 15; n += (n>9?0x37:0x30); console_putc(n); }
                    console_putc(':'); console_putc(' ');
                    ptr = (uint8_t*)(uintptr_t)arg;
                    for(i = 0; i < 16; i++) {
                        n = (ptr[i] >> 4) & 15; n += (n>9?0x37:0x30); console_putc(n);
                        n = ptr[i] & 15; n += (n>9?0x37:0x30); console_putc(n);
                        console_putc(' ');
                    }
                    console_putc(' ');
                    for(i = 0; i < 16; i++)
                        console_putc(ptr[i] < 32 || ptr[i] >= 127 ? '.' : ptr[i]);
                    console_putc('\r'); console_putc('\n');
                    arg += 16;
                } while(--len);
            }
        } else {
put:        console_putc(*fmt);
        }
        fmt++;
    }
    __builtin_va_end(args);
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
    pb_fb = (uint8_t*)vidmode.framebuffer_addr + (vidmode.framebuffer_height - 4) * vidmode.framebuffer_pitch + 2 * pb_b;
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

/**************** BIOS-specific functions ****************/

/* IMPORTANT !!! No function below this line allowed to use more than 1k stack! CLang should generate a warning if they do
 * (actually Clang's stack limit is set to 512 bytes). Also these BIOS routine calls must be in the first 32k in order to work */

uint64_t data_lba, fat_lba;
uint8_t vbr[512], data[512];
uint32_t fat[1024], fat_cache, file_clu;
uint16_t lfn[261];
guid_t bootuuid;

/**
 * Sort memory map
 */
static void sort_map(multiboot_mmap_entry_t *dst, int num)
{
    int i, j;
    uint64_t top;
    multiboot_mmap_entry_t tmp;

    for(i = 1; i < num; i++) {
        for(j = i; j > 0 && dst[j].base_addr < dst[j - 1].base_addr; j--) {
            memcpy(&tmp, &dst[j - 1], sizeof(multiboot_mmap_entry_t));
            memcpy(&dst[j - 1], &dst[j], sizeof(multiboot_mmap_entry_t));
            memcpy(&dst[j], &tmp, sizeof(multiboot_mmap_entry_t));
        }
        top = dst[i].base_addr + dst[i].length;
        if(dst[i].type == MULTIBOOT_MEMORY_AVAILABLE && top > ram) ram = top;
    }
}

/**
 * Load a sector from boot drive using BIOS
 */
void bios_loadsec(uint64_t lba, void *dst)
{
    if(ST || !dst) return;
    *((uint16_t*)0x502) = 1; *((uint32_t*)0x504) = 0x600; *((uint64_t*)0x508) = lba;
    __asm__ __volatile__(
    /* let's see if we have master ATA IDE PIO (probably emulation, but still, it is about 100x times faster than BIOS) */
    "cmpb $0x80, (0x4ff);jne 2f;"
    "movw $0x1F7, %%dx;inb %%dx, %%al;"
    "cmpb $0, %%al;je 2f;"
    "cmpb $0xff, %%al;je 2f;"
    "1:inb %%dx, %%al;"
    "andb $0xC0, %%al;"
    "cmpb $0x40, %%al;jne 1b;"
    "movb $0x0a, %%al;movw $0x3F6, %%dx;outb %%al, %%dx;"
    "movb $0x40, %%al;movw $0x1F6, %%dx;outb %%al, %%dx;"
    "xorb %%al, %%al;movw $0x1F2, %%dx;outb %%al, %%dx;"
    "movq %%rbx, %%rax;shrq $24, %%rax;movw $0x1F3, %%dx;outb %%al, %%dx;"
    "shrq $8, %%rax;movw $0x1F4, %%dx;outb %%al, %%dx;"
    "shrq $8, %%rax;movw $0x1F5, %%dx;outb %%al, %%dx;"
    "movb $1, %%al;movw $0x1F2, %%dx;outb %%al, %%dx;"
    "movq %%rbx, %%rax;movw $0x1F3, %%dx;outb %%al, %%dx;"
    "shrq $8, %%rax;movw $0x1F4, %%dx;outb %%al, %%dx;"
    "shrq $8, %%rax;movw $0x1F5, %%dx;outb %%al, %%dx;"
    "movb $0x24, %%al;movw $0x1F7, %%dx;outb %%al, %%dx;"
    "1:inb %%dx, %%al;"
    "andb $0xC1, %%al;"
    "cmpb $0x01, %%al;je 2f;"
    "cmpb $0x40, %%al;jne 1b;"
    "movq $128, %%rcx;"
    "movw $0x1F0, %%dx;"
    "cld;rep insl;"
    "jmp 3f;"
    /* otherwise fallback to BIOS services, which means switching CPU mode and copy data, so it's slow */
    /* go BIOS mode */
    "2:pushq %%rdi;"
    "movq $0x600, %%rdi;movq $64, %%rcx;xorq %%rax, %%rax;repnz stosq;"
    "movq $16, %%rax;push %%rax;"       /* long -> compat */
    "movq $1f, %%rax;push %%rax;"
    "lretq;.code32;1:;"
    "movl %%cr0, %%eax;"                /* disable CR0.PG */
    "btcl $31, %%eax;"
    "movl %%eax, %%cr0;"
    "ljmp $8,$1f;1:;"                   /* compat -> legacy prot */
    "movl %%cr0, %%eax;"                /* disable CR0.PR */
    "andb $0xfe, %%al;"
    "movl %%eax, %%cr0;"
    "ljmp $0,$1f;.code16;1:;"           /* prot -> real */
    "xorw %%ax, %%ax;movw %%ax, %%ds;movw %%ax, %%es;movw %%ax, %%ss;"
    /* do the BIOS call */
    "movb $0x42, %%ah;"
    "movw $0x500, %%si;"
    "movb -1(%%si), %%dl;"
    "clc;int $0x13;"
    /* go back to long mode */
    "movl %%cr0, %%eax;"                /* enable CR0.PR */
    "orb $1, %%al;"
    "movl %%eax, %%cr0;"
    /* AAAAAHHHHH... we have to workaround a compiler bug... This is a dirty hack that takes advantage of the
     * fact that machine code 0xEA is the same in prot and real encoding just with different operand sizes.
     * smuggle the real mode second operand in prot mode first operand */
    ".code32;ljmp $16,$1f+0x100000;1:;"/* real -> prot */
    "movl %%cr0, %%eax;"                /* enable CR0.PG */
    "btsl $31, %%eax;"
    "movl %%eax, %%cr0;"
    "ljmp $32,$1f;.code64;1:;"          /* prot -> long */
    "movw $40, %%ax;movw %%ax, %%ds;movw %%ax, %%es;movw %%ax, %%ss;"
    /* copy data from BIOS buffer to final position */
    "xorq %%rcx, %%rcx;"
    "xorq %%rsi, %%rsi;"
    "movb $64, %%cl;"
    "movw $0x600, %%si;"
    "popq %%rdi;"
    "cmpq %%rsi, %%rdi;je 3f;"
    "repnz movsq;3:"
    ::"b"(lba),"D"(dst):"rax","rcx","rdx","rsi","memory");
}

/**
 * Set up framebuffer with VESA VBE 2.0 BIOS
 */
void bios_vbe(uint32_t width, uint32_t height, uint32_t bpp)
{
    if(ST || width < 320 || height < 200 || bpp < 15) return;
    memset((void*)0x580, 0, 128);
    __asm__ __volatile__(
    /* go BIOS mode */
    "shlq $32, %%rcx;"
    "shlq $16, %%rbx;"
    "addq %%rbx, %%rcx;"
    "addq %%rax, %%rcx;"
    "xorq %%rax, %%rax;pushq %%rax;"
    "pushq %%rdi;"
    "pushq %%rcx;"
    "movq $16, %%rax;push %%rax;"
    "movq $1f, %%rax;push %%rax;"
    "lretq;.code32;1:;"
    "movl %%cr0, %%eax;"
    "btcl $31, %%eax;"
    "movl %%eax, %%cr0;"
    "ljmp $8,$1f;1:;"
    "movl %%cr0, %%eax;"
    "andb $0xfe, %%al;"
    "movl %%eax, %%cr0;"
    "ljmp $0,$1f;.code16;1:;"
    "xorw %%ax, %%ax;movw %%ax, %%ds;movw %%ax, %%es;movw %%ax, %%ss;"
    /* get EDID information */
    "movw $0x4f15, %%ax;"
    "movw $0x0001, %%bx;"
    "xorw %%cx, %%cx;"
    "xorw %%dx, %%dx;"
    "movw $0x580, %%di;"
    "int $0x10;"
    /* do the VBE BIOS call */
    "movw $0x600, %%di;"
    "movl $0x32454256, 0(%%di);"
    "movw $0x4f00, %%ax;"
    "pushw %%ds;pushw %%es;"
    "int $0x10;"
    "popw %%es;popw %%ds;"
    "cmpw $0x004f, %%ax;jne 2f;"
    /* copy modes list out to workaround buggy VBE BIOSes */
    "xorl %%esi, %%esi;"
    "xorl %%edi, %%edi;"
    "movw (0x60E), %%si;"
    "movw (0x610), %%ax;"
    "movw %%ax, %%ds;"
    "movw $0xA00, %%di;"
    "movw $256, %%cx;"
    "1: lodsw;"
    "cmpw $0xffff, %%ax;je 1f;"
    "cmpw $0, %%ax;je 1f;"
    "stosw;"
    "dec %%cx;jnz 1b;"
    "1:;xorw %%ax, %%ax;stosw;"
    "movw %%ax, %%ds;"
    /* iterate on modes and query info on each */
    "movw $0xffff, 20(%%esp);"          /*   stack:   */
    "movw $0xA00, %%si;"                /* 20(%esp) - best match mode */
    "1:movw $0, (0x600);"               /* 18(%esp) - best match height */
    "lodsw;"                            /* 16(%esp) - best match width */
    "orw %%ax, %%ax;jz 1f;"             /*  8(%esp) - vidmode buffer pointer */
    "movw %%ax, %%cx;"                  /*  4(%esp) - requested bpp */
    "movw $0x600, %%di;"                /*  2(%esp) - requested height */
    "movw $0x4f01, %%ax;"               /*  0(%esp) - requested width */
    "pushw %%cx;pushw %%si;pushw %%ds;pushw %%es;"
    "int $0x10;"
    "popw %%es;popw %%ds;popw %%si;popw %%cx;"
    "cmpw $0x004f, %%ax;jne 1b;"        /* mode listed, but not supported */
    "movw (0x600), %%ax;"
    "andw $0x9b, %%ax;"
    "cmpw $0x9b, %%ax;jne 1b;"          /* not the mode we're looking for. Not packed pixel linear framebuffer */
    "movb 4(%%esp), %%al;"
    "cmpb (0x619), %%al;jne 1b;"        /* bpp matches? */
    "movw (0x614), %%ax;"
    "cmpw 2(%%esp), %%ax;ja 1b;"        /* height smaller or equal than required? */
    "cmpw 18(%%esp), %%ax;jb 1b;"       /* and bigger than the last best result? */
    "movw (0x612), %%ax;"
    "cmpw 0(%%esp), %%ax;ja 1b;"        /* width smaller or equal than required? */
    "cmpw 16(%%esp), %%ax;jb 1b;"       /* and bigger than the last best result? */
    /* this is the best result we have so far, store it */
    "movw %%ax, 16(%%esp);"
    "movw (0x614), %%ax;"
    "movw %%ax, 18(%%esp);"
    "movw %%cx, 20(%%esp);"
    "jmp 1b;"
    "1:cmpw $0xffff, 20(%%esp);je 2f;"
    /* set up mode */
    "movw 20(%%esp), %%bx;"
    "orw $0x6000, %%bx;"
    "movw $0x4f02, %%ax;"
    "pushw %%cx;pushw %%si;pushw %%ds;pushw %%es;"
    "int $0x10;"
    "popw %%es;popw %%ds;popw %%si;popw %%cx;"
    "cmpw $0x004f, %%ax;jne 2f;"        /* not worked, so don't store it */
    /* looks good, store vidmode */
    "movw 20(%%esp), %%cx;"
    "movw $0x600, %%di;"
    "movw $0x4f01, %%ax;"
    "pushw %%cx;pushw %%si;pushw %%ds;pushw %%es;"
    "int $0x10;"
    "popw %%es;popw %%ds;popw %%si;popw %%cx;"
    "cmpw $0x004f, %%ax;jne 2f;"        /* should never happen */
    "movl 8(%%esp), %%edi;"
    "movl (0x628), %%eax;"
    "movl %%eax, 8(%%edi);"             /* vidmode.framebuffer_addr lo */
    "movl (0x62c), %%eax;"
    "movl %%eax, 12(%%edi);"            /* vidmode.framebuffer_addr hi */
    "movw (0x610), %%ax;"
    "movw %%ax, 16(%%edi);"             /* vidmode.framebuffer_pitch */
    "movw (0x612), %%ax;"
    "movw %%ax, 20(%%edi);"             /* vidmode.framebuffer_width */
    "movw (0x614), %%ax;"
    "movw %%ax, 24(%%edi);"             /* vidmode.framebuffer_height */
    "movb (0x619), %%al;"
    "movb %%al, 28(%%edi);"             /* vidmode.framebuffer_bpp */
    "movb $1,   29(%%edi);"             /* vidmode.framebuffer_type */
    "movw (0x61F), %%ax;xchgb %%al, %%ah;"
    "movw %%ax, 32(%%edi);"             /* vidmode.framebuffer_red_field_position + mask */
    "movw (0x621), %%ax;xchgb %%al, %%ah;"
    "movw %%ax, 34(%%edi);"             /* vidmode.framebuffer_green_field_position + mask */
    "movw (0x623), %%ax;xchgb %%al, %%ah;"
    "movw %%ax, 36(%%edi);"             /* vidmode.framebuffer_blue_field_position + mask */
    "2:;"
    /* go back to long mode */
    "movl %%cr0, %%eax;"
    "orb $1, %%al;"
    "movl %%eax, %%cr0;"
    ".code32;ljmp $16,$1f+0x100000;1:;"
    "movl %%cr0, %%eax;"
    "btsl $31, %%eax;"
    "movl %%eax, %%cr0;"
    "ljmp $32,$1f;.code64;1:;"
    "movw $40, %%ax;movw %%ax, %%ds;movw %%ax, %%es;movw %%ax, %%ss;"
    "addq $24, %%rsp;"
    ::"a"(width),"b"(height),"c"(bpp),"D"(&vidmode):"rsi","rdx","memory");
    if(!vidmode.framebuffer_addr) printf(" VESA: no framebuffer\r\n");
#ifdef CONSOLE_FB
    else fb_x = fb_y = 4;
#endif
}

/**
 * Get E820 memory map
 */
int bios_e820(multiboot_mmap_entry_t *dst)
{
    int ret = 0;

    if(ST || !dst) return 0;
    __asm__ __volatile__(
    /* go BIOS mode */
    "pushq %%rdi;xorq %%rdi, %%rdi;pushq $0;"
    "movq $16, %%rax;push %%rax;"
    "movq $1f, %%rax;push %%rax;"
    "lretq;.code32;1:;"
    "movl %%cr0, %%eax;"
    "btcl $31, %%eax;"
    "movl %%eax, %%cr0;"
    "ljmp $8,$1f;1:;"
    "movl %%cr0, %%eax;"
    "andb $0xfe, %%al;"
    "movl %%eax, %%cr0;"
    "ljmp $0,$1f;.code16;1:;"
    "xorw %%ax, %%ax;movw %%ax, %%ds;movw %%ax, %%es;movw %%ax, %%ss;"
    /* do the BIOS call */
    "clc;xorl %%ebx, %%ebx;movl $0x600, %%edi;"
    "1:movl $0xE820, %%eax;"
    "movl $0x534d4150, %%edx;"
    "xorl %%ecx, %%ecx;"
    "movb $20, %%cl;"
    "int $0x15;jc 1f;"
    "addl $20, %%edi;xorl %%eax, %%eax;stosl;"
    "incl 0(%%esp);"
    "or %%ebx, %%ebx;jnz 1b;"
    /* go back to long mode */
    "1:movl %%cr0, %%eax;"
    "orb $1, %%al;"
    "movl %%eax, %%cr0;"
    ".code32;ljmp $16,$1f+0x100000;1:;"
    "movl %%cr0, %%eax;"
    "btsl $31, %%eax;"
    "movl %%eax, %%cr0;"
    "ljmp $32,$1f;.code64;1:;"
    "movw $40, %%ax;movw %%ax, %%ds;movw %%ax, %%es;movw %%ax, %%ss;"
    "movq %%rdi, %%rcx;movq $0x600, %%rsi;subq %%rsi, %%rcx;"
    "popq %%rbx;popq %%rdi;repnz movsb;movq %%rbx, %%rax;"
    :"=a"(ret):"D"(dst):"rbx","rcx","rdx","rsi","memory");
    /* make sure of it that the memory map is sorted. Should be, so bubble-sort is affordable here */
    sort_map(dst, ret);
    if(ret < 1) printf(" E820: unable to get memory map\r\n");
    return ret;
}

/**
 * Chainload fallback to VBR or FreeBSD boot or BIOS boot partition
 */
void bios_fallback(void)
{
    __asm__ __volatile__(
    ".byte 0xe8;.long 0;"               /* relocate real mode code to 0x600 */
    "1:popq %%rsi;addq $2f - 1b, %%rsi;"
    "movq $0x600, %%rdi;"
    "movq $3f - 2f, %%rcx;repnz movsb;"
    "movq $16, %%rax;push %%rax;"       /* long -> compat */
    "movq $1f, %%rax;push %%rax;"
    "lretq;.code32;4:.word 0x3ff;.quad 0;1:;"
    "lidt (4b);"                        /* set up IVT */
    "movl %%cr0, %%eax;"                /* disable CR0.PG */
    "btcl $31, %%eax;"
    "movl %%eax, %%cr0;"
    "ljmp $8,$1f;1:;"                   /* compat -> legacy prot */
    "movl %%cr0, %%eax;"                /* disable CR0.PR */
    "andb $0xfe, %%al;"
    "movl %%eax, %%cr0;"
    "ljmp $0,$0x600;.code16;2:;"        /* prot -> real */
    "xorw %%ax, %%ax;movw %%ax, %%ds;movw %%ax, %%es;movw %%ax, %%ss;"
    "movw $0x7C00, %%bx;movw %%bx, %%sp;"
    "movb $0xF4, (%%bx);" /* move a halt instruction at jump address in case BIOS int fails */
    /* load sectors and jump to the code. We might not have a magic... so nothing to check for */
    "movb $0x42, %%ah;"
    "movw $0x500, %%si;"
    "movb -1(%%si), %%dl;"
    "clc;push %%dx;int $0x13;"          /* load sectors */
    "movw $3, %%ax;int $0x10;pop %%dx;" /* restore VGA text mode / clear screen */
    "cld;cli;ljmp $0, $0x7C00;"
    "3:":::);
}

/**
 * Initialize page tables
 */
void bios_pagetables(int g)
{
    int i;
    memset(&pt[1], 0, 0xFF8);
    memset(&pt[513], 0, 0xFF8);
    memset(&pt[1025], 0, 0x4FF8);
    pt[0] = (uintptr_t)pt + 4096 + 3;
    for(i = 0; i < g; i++) pt[512 + i] = (uintptr_t)pt + (i + 2) * 4096 + 3;
    for(i = 0; i < g * 512; i++) pt[1024 + i] = (uintptr_t)i * 2 * 1024 * 1024 + 0x83;
}

/**************** EFI-specific functions ****************/

efi_handle_t *IM;
efi_boot_services_t *BS;
efi_file_handle_t *root_dir;
efi_file_handle_t *f;
efi_file_info_t info;
memalloc_t ptrs[64];
uint32_t nptr;

/**
 * Set up framebuffer with UEFI GOP
 */
void efi_gop(uint32_t width, uint32_t height, uint32_t bpp)
{
    guid_t gopGuid = EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID;
    efi_gop_t *gop = NULL;
    efi_gop_mode_info_t *info = NULL;
    efi_status_t status;
    uintn_t isiz = sizeof(efi_gop_mode_info_t), i;
    uint32_t m, b = 0, bw = 0, bh = 0, bm = -1U;

    if(!ST || !BS || width < 320 || height < 200 || bpp < 15) return;
    status = BS->LocateProtocol(&gopGuid, NULL, (void**)&gop);
    if(!EFI_ERROR(status) && gop) {
        /* we got the interface, get current mode */
        status = gop->QueryMode(gop, gop->Mode ? gop->Mode->Mode : 0, &isiz, &info);
        if(EFI_ERROR(status) || !gop->Mode)
            status = gop->SetMode(gop, 0);
        if(!EFI_ERROR(status)) {
            /* iterate on modes and find the largest screen with the requested bpp */
            for(i = 0; i < gop->Mode->MaxMode; i++) {
                status = gop->QueryMode(gop, i, &isiz, &info);
                if(EFI_ERROR(status) || info->PixelFormat > PixelBitMask) continue;
                switch(info->PixelFormat) {
                    case PixelRedGreenBlueReserved8BitPerColor:
                    case PixelBlueGreenRedReserved8BitPerColor: b = 32; break;
                    default:
                        for(m = info->PixelInformation.RedMask | info->PixelInformation.GreenMask | info->PixelInformation.BlueMask,
                            b = 32; b > 0 && !(m & (1 << (b - 1))); b--);
                    break;
                }
                if(bpp == b && info->HorizontalResolution <= width && info->VerticalResolution <= height &&
                  info->HorizontalResolution > bw && info->VerticalResolution > bh) {
                    bm = i; bw = info->HorizontalResolution; bh = info->VerticalResolution;
                }
            }
        }
        /* try the best mode that we've found */
        if(bm != -1U) {
            status = gop->SetMode(gop, bm);
            if(!EFI_ERROR(status)) {
#ifdef CONSOLE_FB
                fb_x = fb_y = 4;
#endif
                status = gop->QueryMode(gop, gop->Mode ? gop->Mode->Mode : 0, &isiz, &info);
                switch(info->PixelFormat) {
                    case PixelRedGreenBlueReserved8BitPerColor:
                        vidmode.framebuffer_red_field_position = 0; vidmode.framebuffer_red_mask_size = 8;
                        vidmode.framebuffer_green_field_position = 8; vidmode.framebuffer_green_mask_size = 8;
                        vidmode.framebuffer_blue_field_position = 16; vidmode.framebuffer_blue_mask_size = 8;
                        b = 32;
                    break;
                    case PixelBlueGreenRedReserved8BitPerColor:
                        vidmode.framebuffer_red_field_position = 16; vidmode.framebuffer_red_mask_size = 8;
                        vidmode.framebuffer_green_field_position = 8; vidmode.framebuffer_green_mask_size = 8;
                        vidmode.framebuffer_blue_field_position = 0; vidmode.framebuffer_blue_mask_size = 8;
                        b = 32;
                    break;
                    default:
                        for(m = info->PixelInformation.RedMask | info->PixelInformation.GreenMask | info->PixelInformation.BlueMask,
                            b = 32; b > 0 && !(m & (1 << (b - 1))); b--);
                        for(vidmode.framebuffer_red_field_position = 0;
                            !(info->PixelInformation.RedMask & (1 << vidmode.framebuffer_red_field_position));
                            vidmode.framebuffer_red_field_position++);
                        for(vidmode.framebuffer_red_mask_size = 0;
                            info->PixelInformation.RedMask &
                                (1 << (vidmode.framebuffer_red_field_position + vidmode.framebuffer_red_mask_size));
                            vidmode.framebuffer_red_mask_size++);
                        for(vidmode.framebuffer_green_field_position = 0;
                            !(info->PixelInformation.GreenMask & (1 << vidmode.framebuffer_green_field_position));
                            vidmode.framebuffer_green_field_position++);
                        for(vidmode.framebuffer_green_mask_size = 0;
                            info->PixelInformation.GreenMask &
                                (1 << (vidmode.framebuffer_green_field_position + vidmode.framebuffer_green_mask_size));
                            vidmode.framebuffer_green_mask_size++);
                        for(vidmode.framebuffer_blue_field_position = 0;
                            !(info->PixelInformation.BlueMask & (1 << vidmode.framebuffer_blue_field_position));
                            vidmode.framebuffer_blue_field_position++);
                        for(vidmode.framebuffer_blue_mask_size = 0;
                            info->PixelInformation.BlueMask &
                                (1 << (vidmode.framebuffer_blue_field_position + vidmode.framebuffer_blue_mask_size));
                            vidmode.framebuffer_blue_mask_size++);
                    break;
                }
                vidmode.framebuffer_addr = gop->Mode->FrameBufferBase;
                vidmode.framebuffer_pitch = info->PixelsPerScanLine * ((b + 7) >> 3);
                vidmode.framebuffer_width = info->HorizontalResolution;
                vidmode.framebuffer_height = info->VerticalResolution;
                vidmode.framebuffer_bpp = b;
                vidmode.framebuffer_type = 1;
            }
        }
        if(!vidmode.framebuffer_addr) { printf(" GOP: no framebuffer\r\n"); }
    }
}

/**
 * Get EDID Info
 */
void efi_edid(uint8_t **ptr, uint32_t *size)
{
    guid_t gopGuid = EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID;
    guid_t edid1Guid = EFI_EDID_ACTIVE_GUID;
    guid_t edid2Guid = EFI_EDID_DISCOVERED_GUID;
    efi_edid_t *edid = NULL;
    efi_handle_t *handles = (efi_handle_t*)kernel_buf;
    uintn_t i = sizeof(kernel_buf);

    *size = 0; *ptr = NULL;
    memset(handles, 0, i);
    if(!EFI_ERROR(BS->LocateHandle(ByProtocol, &gopGuid, NULL, &i, handles)) &&
      ((!EFI_ERROR(BS->HandleProtocol(handles[0], &edid1Guid, (void **)&edid)) && edid && edid->SizeOfEdid && edid->Edid) ||
       (!EFI_ERROR(BS->HandleProtocol(handles[0], &edid2Guid, (void **)&edid)) && edid && edid->SizeOfEdid && edid->Edid))) {
        *size = edid->SizeOfEdid; *ptr = edid->Edid;
    }
}

/**
 * Get EFI memory map
 */
int efi_memmap(multiboot_mmap_entry_t *dst)
{
    int ret = 0;
    uint64_t top;
    efi_status_t status;
    efi_memory_descriptor_t *memory_map = NULL, *mement;
    uintn_t memory_map_size = 0, map_key = 0, desc_size = 0;

    if(!ST || !BS) return 0;
    /* get memory map size */
    status = BS->GetMemoryMap(&memory_map_size, NULL, &map_key, &desc_size, NULL);
    if(status != EFI_BUFFER_TOO_SMALL || !memory_map_size) goto err;
    /* allocate buffer. This might increase the memory map's size */
    memory_map_size += 4 * desc_size;
    status = BS->AllocatePool(EfiLoaderData, memory_map_size, (void**)&memory_map);
    if(EFI_ERROR(status) || !memory_map) goto err;
    /* get memory map */
    status = BS->GetMemoryMap(&memory_map_size, memory_map, &map_key, &desc_size, NULL);
    if(EFI_ERROR(status)) { status = BS->FreePool(memory_map); goto err; }
    if(dst) {
        /* convert to Multiboot2 memory entry tags */
        for(mement = memory_map; (uint8_t*)mement < (uint8_t*)memory_map + memory_map_size;
          mement = NextMemoryDescriptor(mement, desc_size), ret++) {
            dst[ret].base_addr = mement->PhysicalStart;
            dst[ret].length = mement->NumberOfPages << 12;
            dst[ret].reserved = mement->Type;
            dst[ret].type = ((mement->Type > EfiReservedMemoryType && mement->Type < EfiRuntimeServicesCode) ||
                mement->Type == EfiConventionalMemory ? MULTIBOOT_MEMORY_AVAILABLE :
                (mement->Type == EfiUnusableMemory ? MULTIBOOT_MEMORY_BADRAM :
                (mement->Type == EfiACPIReclaimMemory ? MULTIBOOT_MEMORY_ACPI_RECLAIMABLE :
                (mement->Type == EfiACPIMemoryNVS ? MULTIBOOT_MEMORY_NVS : MULTIBOOT_MEMORY_RESERVED))));
        }
        /* make sure of it that the memory map is sorted. Should be, so bubble-sort is affordable here */
        sort_map(dst, ret);
    } else {
        /* just iterate through to find the top of memory, aka the amount of RAM we have */
        for(mement = memory_map; (uint8_t*)mement < (uint8_t*)memory_map + memory_map_size;
          mement = NextMemoryDescriptor(mement, desc_size)) {
            top = mement->PhysicalStart + (mement->NumberOfPages << 12);
            if(mement->Type == EfiConventionalMemory && top > ram) ram = top;
        }
    }
    status = BS->FreePool(memory_map);
err:if(dst && ret < 1) printf(" UEFI: unable to get memory map\r\n");
    return ret;
}

/**
 * Allocate and zero out a page on UEFI
 */
uint64_t efi_alloc(void)
{
    efi_status_t status;
    uint64_t page = 0;
    status = BS->AllocatePages(AllocateAnyPages, EfiLoaderData, 1, (efi_physical_address_t*)&page);
    if(EFI_ERROR(status)) page = 0;
    else memset((void*)page, 0, 4096);
    return page;
}

/**
 * Allocate reclaimable memory buffer on UEFI
 */
efi_physical_address_t efi_allocpages(efi_allocate_type_t Type, uintn_t NoPages, efi_physical_address_t Memory)
{
    efi_status_t status;

    if(!ST || !BS) return 0;
    status = BS->AllocatePages(Type, EfiLoaderData, NoPages, (efi_physical_address_t*)&Memory);
    if(EFI_ERROR(status)) Memory = 0;
    else {
        /* we must keep record of the allocated memory ourselves... Good job, UEFI! */
        ptrs[nptr].Memory = Memory;
        ptrs[nptr].NoPages = NoPages;
        nptr++;
    }
    return Memory;
}

/**
 * Free pages
 */
void efi_freepages(void)
{
    uint32_t i;

    if(ST) {
        if(hack_buf) { BS->FreePages(hack_buf, 1024); hack_buf = 0; }
        if(nptr) {
            for(i = 0; i < nptr; i++)
                if(ptrs[i].Memory && ptrs[i].NoPages) BS->FreePages(ptrs[i].Memory, ptrs[i].NoPages);
            nptr = 0; memset(&ptrs, 0, sizeof(ptrs));
        }
    }
}

/**
 * Open a file on UEFI
 */
int efi_open(uint16_t *fn)
{
    efi_status_t status;
    guid_t infGuid = EFI_FILE_INFO_GUID;
    uintn_t fsiz = (uintn_t)sizeof(efi_file_info_t);
    int i;

    if(!ST || !root_dir || !fn || !*fn) return 0;
    for(i = 0; fn[i]; i++) if(fn[i] == '/') fn[i] = '\\';
    status = root_dir->Open(root_dir, &f, fn, EFI_FILE_MODE_READ, 0);
    if(EFI_ERROR(status)) { f = NULL; file_size = 0; return 0; }
    status = f->GetInfo(f, &infGuid, &fsiz, &info);
    file_size = EFI_ERROR(status) ? 0 : (uint64_t)info.FileSize;
    return 1;
}

/**
 * Read data from file on UEFI
 */
uint64_t efi_read(uint64_t offs, uint64_t size, void *buf)
{
    efi_status_t status;
    efi_input_key_t key = { 0 };
    uint64_t blksize, curr;

    if(!ST || !f || offs >= file_size || !size || !buf) return 0;
    if(offs + size > file_size) size = file_size - offs;
    status = f->SetPosition(f, offs);
    if(EFI_ERROR(status)) return 0;
    /* calculate how many bytes to read for one pixel progress. If zero, then no progress bar, read everything in one batch */
    if((blksize = pb_init(size))) {
        for(curr = 0; !EFI_ERROR(status) && curr < size; curr += blksize) {
            /* check for user interruption */
            if(!bkp && !rq) {
                status = ST->ConIn->ReadKeyStroke(ST->ConIn, &key);
                if(!EFI_ERROR(status) && key.UnicodeChar) rq = 1;
            }
            if(size - curr < blksize) blksize = size - curr;
            status = f->Read(f, &blksize, buf + curr);
            pb_draw(curr);
        }
    } else {
        /* check for user interruption */
        if(!bkp && !rq) {
            status = ST->ConIn->ReadKeyStroke(ST->ConIn, &key);
            if(!EFI_ERROR(status) && key.UnicodeChar) rq = 1;
        }
        status = f->Read(f, &size, buf);
    }
    return EFI_ERROR(status) ? 0 : size;
}

/**
 * Close file on UEFI
 */
void efi_close(void)
{
    if(ST && f) f->Close(f);
    f = NULL; file_size = 0;
}

/**
 * Generate tags for system tables on UEFI
 */
void efi_systables(void)
{
    guid_t smGuid = SMBIOS_TABLE_GUID, r1Guid = ACPI_TABLE_GUID, r2Guid = ACPI_20_TABLE_GUID;
    efi_configuration_table_t *tbl;
    uint32_t i;
    uint8_t *s;

    if(!ST || !tags_ptr) return;
    for(i = 0, tbl = ST->ConfigurationTable; i < ST->NumberOfTableEntries; i++, tbl++) {
        if(!memcmp(&tbl->VendorGuid, &smGuid, sizeof(guid_t))) {
            s = tbl->VendorTable;
            memset(tags_ptr, 0, sizeof(multiboot_tag_smbios_t));
            ((multiboot_tag_smbios_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_SMBIOS;
            ((multiboot_tag_smbios_t*)tags_ptr)->size = sizeof(multiboot_tag_smbios_t) + (uint32_t)s[5];
            ((multiboot_tag_smbios_t*)tags_ptr)->major = s[7];
            ((multiboot_tag_smbios_t*)tags_ptr)->minor = s[8];
            memcpy(((multiboot_tag_smbios_t*)tags_ptr)->tables, s, (uint32_t)s[5]);
            tags_ptr += (((multiboot_tag_smbios_t*)tags_ptr)->size + 7) & ~7;
        } else
        if(!memcmp(&tbl->VendorGuid, &r2Guid, sizeof(guid_t))) {
            s = tbl->VendorTable;
            ((multiboot_tag_new_acpi_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_ACPI_NEW;
            ((multiboot_tag_new_acpi_t*)tags_ptr)->size = sizeof(multiboot_tag_new_acpi_t) + 36;
            memcpy(((multiboot_tag_new_acpi_t*)tags_ptr)->rsdp, s, 36);
            tags_ptr += (((multiboot_tag_new_acpi_t*)tags_ptr)->size + 7) & ~7;
            rsdp_ptr = (uintptr_t)s;
        }
    }
    /* only generate old ACPI tag if 64-bit RSDP wasn't found */
    for(i = 0, tbl = ST->ConfigurationTable; i < ST->NumberOfTableEntries && !rsdp_ptr; i++, tbl++)
        if(!memcmp(&tbl->VendorGuid, &r1Guid, sizeof(guid_t))) {
            s = tbl->VendorTable;
            ((multiboot_tag_old_acpi_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_ACPI_OLD;
            ((multiboot_tag_old_acpi_t*)tags_ptr)->size = sizeof(multiboot_tag_old_acpi_t) + 24;
            memcpy(((multiboot_tag_old_acpi_t*)tags_ptr)->rsdp, s, 24);
            tags_ptr += (((multiboot_tag_old_acpi_t*)tags_ptr)->size + 7) & ~7;
            rsdp_ptr = (uintptr_t)s;
        }
}

/**
 * Initialize UEFI related things
 */
void efi_init(void)
{
    efi_loaded_image_protocol_t *LIP = NULL;
    efi_simple_file_system_protocol_t *sfs = NULL;
    guid_t dppGuid = EFI_DEVICE_PATH_PROTOCOL_GUID;
    guid_t lipGuid = EFI_LOADED_IMAGE_PROTOCOL_GUID;
    guid_t sfsGuid = EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID;
    uint8_t *buf = NULL, *ptr, *end;

    BS = ST->BootServices;
    /* we don't need this buffer, we just need this area to be marked as used in the memory map, so that
     * subsequent allocations won't pollute it. If UEFI already occupies this area, that sucks, there's
     * nothing we can do about it and would most likely result in "memory already in use" errors in
     * fw_loadseg. Otherwise this memory is freed in efi_freepages called from fw_loadkernel. */
    hack_buf = file_buf;
    if(EFI_ERROR(BS->AllocatePages(AllocateAddress, EfiLoaderData, 1024, (efi_physical_address_t*)&hack_buf))) hack_buf = 0;
    /* set up framebuffer */
    efi_gop(fb_w, fb_h, fb_bpp);
    if(!vidmode.framebuffer_addr) { fb_w = 640; fb_h = 480; efi_gop(fb_w, fb_h, fb_bpp); }
    if(IM && BS && BS->HandleProtocol) {
        BS->HandleProtocol(IM, &lipGuid, (void **)&LIP);
        if(!EFI_ERROR(BS->HandleProtocol(LIP->DeviceHandle, &sfsGuid, (void **)&sfs))) {
            /* get boot partition's root directory */
            if(EFI_ERROR(sfs->OpenVolume(sfs, &root_dir))) root_dir = NULL;
        }
        /* get the ESP's device path */
        if(!EFI_ERROR(BS->HandleProtocol(LIP->DeviceHandle, &dppGuid, (void **)&buf)) && buf) {
            /* also get the boot partition's UniquePartitionGUID */
            for(ptr = buf, end = buf + 32768; ptr < end && *ptr != 0x7F && (ptr[0] != 4 || ptr[1] != 1); ptr += (ptr[3] << 8) | ptr[2]);
            if(*ptr == 4) memcpy(&bootuuid, &((efi_hard_disk_device_path_t*)ptr)->PartitionSignature, sizeof(guid_t));
            BS->FreePool(buf);
        }
    }
    /* if this fails, that sucks, because we don't have a console yet. We'll report it later */
    pt = NULL;
    if(EFI_ERROR(BS->AllocatePages(AllocateAnyPages, EfiLoaderData, 68, (efi_physical_address_t*)&pt))) pt = NULL;
    /* otherwise set up the same page tables we have with BIOS, but don't active it yet */
    else bios_pagetables(64);
}

/**
 * Free all EFI related buffers
 */
void efi_freeall(void)
{
    if(!ST || !BS) return;
    if(conf_buf) { BS->FreePool(conf_buf); conf_buf = NULL; }
    if(logo_buf) { BS->FreePool(logo_buf); logo_buf = NULL; }
    if(hack_buf) { BS->FreePages(hack_buf, 1024); hack_buf = 0; }
}

/* we can't include this sooner, because the routines above must be in the first 32k. This is because we must
 * access those from real mode, and we're loaded at 0x8000, and the real mode segment's limit is 0xFFFF bytes. */
#include "inflate.h"

/**
 * Generate tags for system tables on FOSSBIOS
 */
void fb_systables(void)
{
    rsdp_t *rsdp;
    int i, s;

    if(ST || !FB) return;

    /* create a fake ACPI table */
    ((multiboot_tag_old_acpi_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_ACPI_OLD;
    ((multiboot_tag_old_acpi_t*)tags_ptr)->size = sizeof(multiboot_tag_old_acpi_t) + sizeof(rsdp_t);
    rsdp = (rsdp_t*)&((multiboot_tag_old_acpi_t*)tags_ptr)->rsdp;
    memset(rsdp, 0, sizeof(rsdp_t));
    memcpy(&rsdp->magic, "RSD PTR ", 8);
    rsdp->rev = 1; rsdp->rsdt = (uint32_t)(uintptr_t)&rsdt;
    for(s = 0, i = 0; i < (int)sizeof(rsdp_t); i++) { s += *(((uint8_t*)rsdp) + i); } rsdp->chksum = 0x100 - s;
    memset(&rsdt, 0, sizeof(rsdt_t));
    memcpy(&rsdt.hdr.magic, "RSDT", 4);
    rsdt.hdr.rev = 1; rsdt.hdr.size = sizeof(sdt_hdr_t) + 1 * sizeof(uint32_t);
    /* add fake FADT and DSDT tables to the ACPI list with the GUDT data */
    rsdt.table_ptr[0] = (uint32_t)(uintptr_t)&fadt; /* DSDT is pointed by FADT, not RSDT */
    memset(&fadt, 0, sizeof(fadt_t));
    memcpy(&fadt.hdr.magic, "FACP", 4);
    fadt.hdr.rev = 2; fadt.hdr.size = sizeof(fadt_t); fadt.dsdt = (uint32_t)(uintptr_t)FB->conf; fadt.x_dsdt = (uintptr_t)FB->conf;
    for(s = 0, i = 0; i < (int)sizeof(fadt); i++) { s += *(((uint8_t*)&fadt) + i); } fadt.hdr.chksum = 0x100 - s;
    for(s = 0, i = 0; i < (int)sizeof(rsdt_t); i++) { s += *(((uint8_t*)&rsdt) + i); } rsdt.hdr.chksum = 0x100 - s;
    tags_ptr += (((multiboot_tag_old_acpi_t*)tags_ptr)->size + 7) & ~7;
}

/**
 * Get FOSSBIOS memory map
 */
int fb_memmap(multiboot_mmap_entry_t *dst)
{
    int ret, i, j;
    uint64_t top;
    multiboot_mmap_entry_t tmp;
    fb_mement_t *mement;

    if(ST || !FB || !dst) return 0;

    ret = FB->system->nummap;
    mement = FB->system->memmap;
    for(i = 0; i < ret; i++) {
        dst[i].base_addr = mement[i].base;
        dst[i].length = mement[i].size & ~7;
        dst[i].reserved = mement[i].size & 7;
        switch(dst[i].reserved) {
            case FB_MEM_FREE:
            case FB_MEM_USED:
            case FB_MEM_BIOS: dst[i].type = MULTIBOOT_MEMORY_AVAILABLE; break;
            case FB_MEM_ROM:
            case FB_MEM_MMIO: dst[i].type = MULTIBOOT_MEMORY_RESERVED; break;
            case FB_MEM_NVS: dst[i].type = MULTIBOOT_MEMORY_NVS; break;
            default: dst[i].type = MULTIBOOT_MEMORY_BADRAM; break;
        }
    }
    /* make sure of it that the memory map is sorted. Should be, so bubble-sort is affordable here */
    for(i = 1; i < ret; i++) {
        for(j = i; j > 0 && dst[j].base_addr < dst[j - 1].base_addr; j--) {
            memcpy(&tmp, &dst[j - 1], sizeof(multiboot_mmap_entry_t));
            memcpy(&dst[j - 1], &dst[j], sizeof(multiboot_mmap_entry_t));
            memcpy(&dst[j], &tmp, sizeof(multiboot_mmap_entry_t));
        }
        top = dst[i].base_addr + dst[i].length;
        if(dst[i].type == MULTIBOOT_MEMORY_AVAILABLE && top > ram) ram = top;
    }
    if(ret < 1) printf(" SYS: unable to get memory map\r\n");
    return ret;
}

/**
 * Set up linear framebuffer with FOSSBIOS
 */
void fb_lfb(uint32_t width, uint32_t height, uint32_t bpp)
{
    fb_vidmode_t *mode;
    uint32_t i, bw = 0, bh = 0, bm = -1U;

    if(ST || !FB || !FB->video || width < 320 || height < 200 || bpp < 15) return;
    /* iterate on modes and find the largest screen with the requested bpp */
    mode = FB->video->vidmodes;
    for(i = 0; i < FB->video->nummodes; i++, mode++)
        if(bpp == mode->bpp && mode->width <= width && mode->height <= height && mode->width > bw && mode->height > bh) {
            bm = i; bw = mode->width; bh = mode->height;
        }
    /* try the best mode that we've found */
    if(bm != -1U && FB->video->setmode(bm)) {
#ifdef CONSOLE_FB
        fb_x = fb_y = 4;
#endif
        mode = &FB->video->vidmodes[bm];
        switch(mode->bpp) {
            case 15:
                vidmode.framebuffer_red_mask_size = vidmode.framebuffer_green_mask_size = vidmode.framebuffer_blue_mask_size = 5;
            break;
            case 16:
                vidmode.framebuffer_red_mask_size = vidmode.framebuffer_blue_mask_size = 5; vidmode.framebuffer_green_mask_size = 6;
            break;
            default:
                vidmode.framebuffer_red_mask_size = vidmode.framebuffer_green_mask_size = vidmode.framebuffer_blue_mask_size = 8;
            break;
        }
        vidmode.framebuffer_red_field_position = mode->red;
        vidmode.framebuffer_green_field_position = mode->green;
        vidmode.framebuffer_blue_field_position = mode->blue;
        vidmode.framebuffer_addr = (uintptr_t)FB->video->lfb;
        vidmode.framebuffer_pitch = mode->pitch;
        vidmode.framebuffer_width = mode->width;
        vidmode.framebuffer_height = mode->height;
        vidmode.framebuffer_bpp = mode->bpp;
        vidmode.framebuffer_type = 1;
    }
    if(!vidmode.framebuffer_addr) { printf(" LFB: no framebuffer\r\n"); }
}

/**
 * Set up framebuffer with legacy BIOS or FOSSBIOS
 */
void fw_lfb(uint32_t width, uint32_t height, uint32_t bpp)
{
    FB ? fb_lfb(width, height, bpp) : bios_vbe(width, height, bpp);
}

/**
 * Load a sector from legacy BIOS or FOSSBIOS
 */
void fw_loadsec(uint64_t lba, void *dst)
{
    uint8_t al;

    if(FB) {
        if(!bkp && !rq && FB->input && FB->input->haskey()) rq = 1;
        if(FB->storage) FB->storage->read(bootdev, 512, lba, dst);
    } else {
        if(!bkp && !rq) {
            __asm__ __volatile__("inb $0x64, %%al;":"=a"(al)::);
            if(al != 0xFF && al & 1) rq = 1;
        }
        bios_loadsec(lba, dst);
    }
}

/**
 * Sleep 1 usec
 */
void bios_sleep(void)
{
            /* NOTE: this should be the right solution, as it only relies on PS/2 alone and needs nothing else. However qemu's
             * PS/2 emulation is buggy, the oscillation is host freq dependent and not constant as it is on a real hardware. */
#if 0
            /* wait a bit, PS/2 control port's bit 4 is oscillating at 15 usecs, use that (67 * 15 = 1005 usecs) */
            __asm__ __volatile__ ("inb $0x61,%%al;andb $0x10,%%al;movb %%al,%%ah;movw $67, %%cx;"
            "1:;inb $0x61,%%al;andb $0x10,%%al;cmpb %%al,%%ah;je 1b;movb %%al,%%ah;nop;pause;dec %%cx;jnz 1b;" : : : "rax", "rbx", "rcx", "rdx");
#else
            /* wait a bit, polling the PIT latch for delay. 1000 usec = 1,193,182 Hz / 1000 ms = 1193 */
            __asm__ __volatile__ ("movw $1193, %%cx;"                                                       /* cx = ticks to wait */
            "xor %%al,%%al;outb %%al,$0x43;inb $0x40,%%al;movb %%al,%%bl;inb $0x40,%%al;movb %%al,%%bh;"    /* bx = last counter */
            "1:xor %%al,%%al;outb %%al,$0x43;inb $0x40,%%al;movb %%al,%%ah;inb $0x40,%%al;xchgb %%al,%%ah;" /* ax = current counter */
            "nop;pause;subw %%ax,%%bx;subw %%bx,%%cx;movw %%ax,%%bx;jae 1b;" /* cx -= (bx - ax); bx = ax; while(cx > 0); */
             : : : "rax", "rbx", "rcx");
#endif
}

/**
 * Allocate and zero out a page on BIOS
 */
uint64_t bios_alloc(void)
{
    uint64_t page = file_buf;
    file_buf += 4096;
    memset((void*)page, 0, 4096);
    return page;
}

/**
 * Get the next cluster from FAT
 */
uint32_t bios_nextclu(uint32_t clu)
{
    uint64_t i;

    if(ST || clu < 2 || clu >= 0x0FFFFFF8) return 0;
    if(clu < fat_cache || clu > fat_cache + 1023) {
        fat_cache = clu & ~1023;
        for(i = 0; i < 8; i++) fw_loadsec(fat_lba + (fat_cache >> 7) + i, &fat[i << 7]);
    }
    clu = fat[clu - fat_cache];
    return clu < 2 || clu >= 0x0FFFFFF8 ? 0 : clu;
}

/**
 * Open a file on BIOS
 */
int bios_open(uint16_t *fn)
{
    uint64_t lba;
    uint32_t clu = bpb->rc;
    int i, n = 0, m = 0;
    uint8_t secleft = 0, *dir = data + sizeof(data);
    uint16_t *u, *s = fn, a, b;

    if(ST || !root_dir || !fn || !*fn) return 0;
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
                clu = bios_nextclu(clu);
            }
            fw_loadsec(lba, &data);
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
 * Read data from file on BIOS
 */
uint64_t bios_read(uint64_t offs, uint64_t size, void *buf)
{
    uint64_t lba = 0, rem, o;
    uint32_t clu = file_clu, nc, ns = 0, os = 0, rs = 512;
    uint8_t secleft = 0;

    if(ST || file_clu < 2 || offs >= file_size || !size || !buf) return 0;
    if(offs + size > file_size) size = file_size - offs;
    rem = size;

    pb_init(size);
    if(offs) {
        nc = offs / (bpb->spc << 9); o = offs % (bpb->spc << 9);
        ns = o >> 9; os = o & 0x1ff; rs = 512 - os;
        if(nc) { while(nc-- && clu) { clu = bios_nextclu(clu); } if(!clu) return 0; }
        secleft = bpb->spc - ns - 1;
        lba = clu * bpb->spc + ns - 1 + data_lba;
    }
    while(rem && !rq) {
        if(secleft) { secleft--; lba++; }
        else {
            if(!clu) break;
            secleft = bpb->spc - 1;
            lba = clu * bpb->spc + data_lba;
            clu = bios_nextclu(clu);
        }
        if(rs > rem) rs = rem;
        if(rs < 512) {
            fw_loadsec(lba, data);
            memcpy(buf, data + os, rs); os = 0;
        } else {
            fw_loadsec(lba, buf);
            if(os) { memcpy(buf, buf + os, rs); os = 0; }
        }
        buf += rs; rem -= rs; rs = 512;
        pb_draw(size - rem);
    }
    pb_fini();
    return (size - rem);
}

/**
 * Close file on BIOS
 */
void bios_close(void)
{
    file_clu = file_size = 0;
}

/**
 * Generate tags for system tables on BIOS
 */
void bios_systables(void)
{
    int sm = 0, rp = 0, i;
    uint8_t *s, chk;

    for(s = (uint8_t*)0x9A000; s < (uint8_t*)0x100000 && !(sm & rp); s += 16) {
        if(!memcmp(s, "_SM_", 4)) {
            for(chk = 0, i = 0; (uint32_t)i < (uint32_t)s[5]; i++) chk += s[i];
            if(!chk) {
                memset(tags_ptr, 0, sizeof(multiboot_tag_smbios_t));
                ((multiboot_tag_smbios_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_SMBIOS;
                ((multiboot_tag_smbios_t*)tags_ptr)->size = sizeof(multiboot_tag_smbios_t) + (uint32_t)s[5];
                ((multiboot_tag_smbios_t*)tags_ptr)->major = s[7];
                ((multiboot_tag_smbios_t*)tags_ptr)->minor = s[8];
                memcpy(((multiboot_tag_smbios_t*)tags_ptr)->tables, s, (uint32_t)s[5]);
                tags_ptr += (((multiboot_tag_smbios_t*)tags_ptr)->size + 7) & ~7;
            }
            sm = 1;
        } else
        if(!memcmp(s, "RSD PTR ", 8)) {
            if(s[15] < 2) {
                ((multiboot_tag_old_acpi_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_ACPI_OLD;
                ((multiboot_tag_old_acpi_t*)tags_ptr)->size = sizeof(multiboot_tag_old_acpi_t) + 24;
                memcpy(((multiboot_tag_old_acpi_t*)tags_ptr)->rsdp, s, 24);
                tags_ptr += (((multiboot_tag_old_acpi_t*)tags_ptr)->size + 7) & ~7;
            } else {
                ((multiboot_tag_new_acpi_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_ACPI_NEW;
                ((multiboot_tag_new_acpi_t*)tags_ptr)->size = sizeof(multiboot_tag_new_acpi_t) + 36;
                memcpy(((multiboot_tag_new_acpi_t*)tags_ptr)->rsdp, s, 36);
                tags_ptr += (((multiboot_tag_new_acpi_t*)tags_ptr)->size + 7) & ~7;
            }
            rsdp_ptr = (uintptr_t)s;
            rp = 1;
        }
    }
}

/**
 * Initialize BIOS related things
 */
void bios_init(void)
{
    guid_t espGuid = EFI_PART_TYPE_EFI_SYSTEM_PART_GUID;
    guid_t fbbGuid = { 0x83bd6b9d, 0x7f41, 0x11dc, { 0xbe, 0x0b, 0x00, 0x15, 0x60, 0xb8, 0x4f, 0x0f } };
    guid_t bbpGuid = { 0x21686148, 0x6449, 0x6e6f, { 0x74, 0x4e, 0x65, 0x65, 0x64, 0x45, 0x46, 0x49 } };
    uint64_t i, j, k, l, n;

    /* finish up the page tables (up to 5G to be safe) */
    /* We couldn't do this sooner, as this overwrites the PMBR code at 0x7C00 */
    pt = (uint64_t*)0x1000;
    bios_pagetables(5);
    __asm__ __volatile__("movq %%rax, %%cr3"::"a"(0x1000):);
    fw_lfb(fb_w, fb_h, fb_bpp);
    if(!vidmode.framebuffer_addr) {
        fb_w = 800; fb_h = 600; fw_lfb(fb_w, fb_h, fb_bpp);
        if(!vidmode.framebuffer_addr) { fb_w = 640; fb_h = 480; fw_lfb(fb_w, fb_h, fb_bpp); }
    }

    /* get boot partition's root directory */
    fw_loadsec(1, &vbr);
    if(!memcmp(&vbr, EFI_PTAB_HEADER_ID, 8)) {
        /* found GPT */
        j = ((gpt_header_t*)&vbr)->SizeOfPartitionEntry;
        l = ((gpt_header_t*)&vbr)->PartitionEntryLBA;
        n = ((gpt_header_t*)&vbr)->NumberOfPartitionEntries;
        /* look for ESP in the first 8 sectors only. Should be the very first entry anyway */
        for(k = 0; k < 8 && n; k++) {
            fw_loadsec(l + k, &vbr);
            for(i = 0; i + j <= 512; i += j, n--) {
                /* does ESP type match? */
                if(!root_dir && !memcmp(&((gpt_entry_t*)&vbr[i])->PartitionTypeGUID, &espGuid, sizeof(guid_t))) {
                    root_dir = (void*)(((gpt_entry_t*)&vbr[i])->StartingLBA);
                    memcpy(&bootuuid, &(((gpt_entry_t*)&vbr[i])->UniquePartitionGUID), sizeof(guid_t));
                } else
                /* look for fallback option: FreeBSD boot or BIOS boot partition type? */
                if(!vbr_lba && (
                  !memcmp(&((gpt_entry_t*)&vbr[i])->PartitionTypeGUID, &fbbGuid, sizeof(guid_t)) ||
                  !memcmp(&((gpt_entry_t*)&vbr[i])->PartitionTypeGUID, &bbpGuid, sizeof(guid_t)))) {
                    vbr_lba = ((gpt_entry_t*)&vbr[i])->StartingLBA;
                    vbr_size = ((gpt_entry_t*)&vbr[i])->EndingLBA - ((gpt_entry_t*)&vbr[i])->StartingLBA + 1;
                } else
                /* look for fallback option: maybe marked for legacy boot? (should override part type fallbacks) */
                if(((gpt_entry_t*)&vbr[i])->Attributes & EFI_PART_USED_BY_OS && ((gpt_entry_t*)&vbr[i])->StartingLBA > 2) {
                    vbr_lba = ((gpt_entry_t*)&vbr[i])->StartingLBA;
                    vbr_size = 1;
                }

            }
        }
    } else {
        /* fallback to MBR partitioning scheme */
        fw_loadsec(0, &vbr);
        if(vbr[510] == 0x55 && vbr[511] == 0xAA)
            for(i = 0x1c0; i < 510; i += 16)
                if(vbr[i - 2] == 0x80/*active*/ && (vbr[i + 2] == 0xC/*FAT32*/ || vbr[i + 2] == 0xEF/*ESP*/)) {
                    root_dir = (void*)(uint64_t)(*((uint32_t*)&vbr[i + 6]));
                    memcpy(&bootuuid.Data1, "PART", 4); memcpy(bootuuid.Data4, "boot", 4);
                    bootuuid.Data2 = *((uint8_t*)0x4ff); bootuuid.Data3 = (i - 0x1c0) / 16;
                    break;
                }
    }
    /* we shamelessly reuse the pointer to store the boot partition's start LBA, because later we use that as
     * a flag to see if we have found a file system, otherwise we add it to data_lba and never use it again */
    if(root_dir) {
        fw_loadsec((uint64_t)root_dir, &vbr);
        bpb = (esp_bpb_t*)&vbr;
        if(vbr[510] != 0x55 || vbr[511] != 0xAA || bpb->bps != 512 || !bpb->spc || bpb->spf16 || !bpb->spf32)
            root_dir = NULL;
        else {
            /* calculate the LBA address of the FAT and the first data sector */
            fat_lba = bpb->rsc + (uint64_t)root_dir;
            data_lba = bpb->spf32 * bpb->nf + bpb->rsc - 2 * bpb->spc + (uint64_t)root_dir;
            /* load the beginning of the FAT into the cache */
            for(i = 0; i < 8; i++) fw_loadsec(fat_lba + i, &fat[i << 7]);
            fat_cache = 0;
        }
    }
}

/**************** Common functions ****************/

/**
 * Initialize firmware related stuff
 */
void fw_init(efi_handle_t image, efi_system_table_t *systab, uint16_t bdev)
{
    /* make sure SSE is enabled, because some say there are buggy firmware in the wild not enabling (and also needed if we come
     * from boot_x86.asm). No supported check, because according to AMD64 Spec Vol 2, all long mode capable CPUs must also
     * support SSE2 at least. We don't need them, but it's more than likely that a kernel is compiled using SSE instructions. */
    __asm__ __volatile__ (
    "movq %%cr0, %%rax;andb $0xF1, %%al;movq %%rax, %%cr0;"     /* clear MP, EM, TS (FPU emulation off) */
    "movq %%cr4, %%rax;orw $3 << 9, %%ax;movq %%rax, %%cr4;"    /* set OSFXSR, OSXMMEXCPT (enable SSE) */
    :::"rax");

    /* the default framebuffer resolution. We set this up anyway so that we can display console messages
     * on screen, but later this will be changed to whatever the user requested. */
    fb_w = 800; fb_h = 600; fb_bpp = 32;

    /* initialize everything to zero */
    IM = 0; ST = NULL; FB = NULL; bootdev = 0;
    if((uint32_t)(uintptr_t)image == 0xF055B105) {
        FB = (fossbios_t*)systab;
        if(bdev < (FB->storage ? FB->storage->num : 0)) bootdev = bdev;
    } else {
        ST = systab;
        IM = image;
    }
    root_dir = NULL; BS = NULL; bpb = NULL; f = NULL; memmap = NULL; initrd = NULL; num_memmap = verbose = bkp = rq = nptr = smp = 0;
    zero_page = NULL; memset(&ptrs, 0, sizeof(ptrs));
    conf_buf = kernel = cmdline = NULL; kernel_entry = logo_buf = tags_ptr = NULL; kernel_mode = MODE_MB64;
    rsdp_ptr = mod_buf = dsdt_ptr = ram = vbr_lba = vbr_size = hack_buf = 0; in_exc = 0;
    memset(&bootuuid, 0, sizeof(guid_t)); memset(&vidmode, 0, sizeof(vidmode)); fb_bg = 0; pb_fb = NULL;
    /* things with fixed address */
    tags_buf = (uint8_t*)0x20000; file_buf = 0x100000;

    /* do firmware specific initialization */
    if(ST) efi_init(); else bios_init();
    if(!vidmode.framebuffer_addr) vidmode.framebuffer_width = vidmode.framebuffer_height = vidmode.framebuffer_bpp = 0;
    console_init();
}

/**
 * Display a boot splash
 */
void fw_bootsplash(void)
{
    uint32_t c;
    uint8_t *fb = (uint8_t*)vidmode.framebuffer_addr;
    int i, j, k, l, x, y, w, h, o, m, p, px, py, b = (vidmode.framebuffer_bpp + 7) >> 3;

    /* clear screen */
    if(!fb) return;
    for(j = y = 0; y < (int)vidmode.framebuffer_height; y++, j += vidmode.framebuffer_pitch)
        for(i = j, x = 0; x < (int)vidmode.framebuffer_width; x++, i += b)
            if(b == 2) *((uint16_t*)(fb + i)) = (uint16_t)fb_bg; else *((uint32_t*)(fb + i)) = fb_bg;
#ifdef CONSOLE_FB
    fb_x = fb_y = 4;
#endif
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
    uint16_t *d, c;
    char *s;

    if(!root_dir || !fn || !*fn) return 0;
    /* UTF-8 to WCHAR */
    for(s = fn, d = wcname; *s && *s != ' ' && *s != '\r' && *s != '\n' && d < &wcname[PATH_MAX - 2]; d++) {
        if((*s & 128) != 0) {
            if(!(*s & 32)) { c = ((*s & 0x1F)<<6)|(*(s+1) & 0x3F); s++; } else
            if(!(*s & 16)) { c = ((*s & 0xF)<<12)|((*(s+1) & 0x3F)<<6)|(*(s+2) & 0x3F); s += 2; } else
            if(!(*s & 8)) { c = ((*s & 0x7)<<18)|((*(s+1) & 0x3F)<<12)|((*(s+2) & 0x3F)<<6)|(*(s+3) & 0x3F); *s += 3; }
            else c = 0;
        } else c = *s;
        s++; if(c == '\\' && *s == ' ') { c = ' '; s++; }
        *d = c;
    }
    *d = 0;
    return ST ? efi_open(wcname) : bios_open(wcname);
}

/**
 * Read data from file
 */
uint64_t fw_read(uint64_t offs, uint64_t size, void *buf)
{
    if(!root_dir || offs >= file_size || !size || !buf) return 0;
    return ST ? efi_read(offs, size, buf) : bios_read(offs, size, buf);
}

/**
 * Close file
 */
void fw_close(void)
{
    ST ? efi_close() : bios_close();
}

/**
 * Load and parse config (everything except modules)
 */
void fw_loadconfig(void)
{
    efi_status_t status;
    char *s, *e, *a;
    uint32_t r, g, b;
    int l, m = 0;

    if(bkp) { fw_bootsplash(); printf("Aborted, loading backup configuration...\r\n"); }

    kernel = NULL;
    tags_buf = (uint8_t*)0x20000;
    /* as a fallback, we try to load the first menuentry from easyboot's configuration */
    if(fw_open("simpleboot.cfg") || (!bkp && fw_open("easyboot/menu.cfg"))) {
        if(ST) {
            if(!conf_buf) {
                status = BS->AllocatePool(EfiLoaderData, file_size + 1, (void**)&conf_buf);
                if(EFI_ERROR(status) || !conf_buf) { fw_close(); goto err; }
            }
        } else {
            conf_buf = (char*)tags_buf;
            tags_buf += (file_size + 7) & ~7;
        }
        fw_read(0, file_size, conf_buf);
        conf_buf[file_size] = 0;
        fw_close();
        fb_w = vidmode.framebuffer_width; fb_h = vidmode.framebuffer_height; fb_bpp = vidmode.framebuffer_bpp; fb_bg = 0; smp = 0;
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
                a = getint(a, &fb_w); while(a < e && *a == ' ') a++;
                a = getint(a, &fb_h); while(a < e && *a == ' ') a++;
                a = getint(a, &fb_bpp);
                if(fb_w < 320 || fb_w > 65536 || fb_h < 200 || fb_h > 65536 || fb_bpp < 15 || fb_bpp > 32) {
                    fb_w = vidmode.framebuffer_width; fb_h = vidmode.framebuffer_height; fb_bpp = vidmode.framebuffer_bpp;
                }
            } else
            if(!memcmp(s, "bootsplash", 10)) {
                if(*a == '#') {
                    a++; a = gethex(a, &r); a = gethex(a, &g); a = gethex(a, &b);
                    fb_bg = FB_COLOR(r, g, b);
                    while(a < e && *a == ' ') a++;
                }
                if(a < e) {
                    if(fw_open(a)) {
                        if(ST) {
                            if(logo_buf) { BS->FreePool(logo_buf); logo_buf = NULL; }
                            status = BS->AllocatePool(EfiLoaderData, file_size, (void**)&logo_buf);
                            if(EFI_ERROR(status)) logo_buf = NULL;
                        } else {
                            logo_buf = tags_buf;
                            tags_buf += (file_size + 7) & ~7;
                        }
                        if(verbose) printf("Loading logo '%S' (%ld bytes)...\r\n", wcname, file_size);
                        logo_size = file_size;
                        fw_read(0, file_size, logo_buf);
                        fw_close();
                    } else if(verbose) printf("WARNING: unable to load '%S'\r\n", wcname);
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
err:if(!kernel) kernel = (char*)defkernel;
    if(!bkp && (volatile char)definitrd[63] == 1) smp = 1;
}

/**
 * Detect config file independent configuration and generate tags for them
 */
void fw_loadsetup()
{
    efi_status_t status;
    multiboot_tag_loader_t *stag;
    multiboot_tag_mmap_t *mtag;
    char *c;

    if(!ST) {
        mod_buf = 0;
        file_buf = 0x100000;
    }
    if(!bkp && ST) {
        tags_buf = NULL;
        status = BS->AllocatePages(AllocateAnyPages, EfiLoaderData, TAGS_MAX, (efi_physical_address_t*)&tags_buf);
        if(EFI_ERROR(status)) tags_buf = NULL;
    }
    tags_ptr = tags_buf;
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
        /* get memory map early, because we'll need it on BIOS to decide where to load the kernel
         * for UEFI this must be done as the last step, because the memory map will change during boot */
        if(!ST) {
            /* get system tables and generate tags for them */
            FB ? fb_systables() : bios_systables();
            /* generate memory map tag */
            mtag = (multiboot_tag_mmap_t*)tags_ptr;
            mtag->type = MULTIBOOT_TAG_TYPE_MMAP;
            mtag->entry_size = sizeof(multiboot_mmap_entry_t);
            mtag->entry_version = 0;
            num_memmap = FB ? fb_memmap(mtag->entries) : bios_e820(mtag->entries);
            if(num_memmap > 0) {
                memmap = mtag->entries;
                mtag->size = sizeof(multiboot_tag_mmap_t) + num_memmap * sizeof(multiboot_mmap_entry_t);
                tags_ptr += (mtag->size + 7) & ~7;
            }
        } else {
            /* get system tables and generate tags for them */
            efi_systables();
            /* we can't use it, however we still have to query the memory map on UEFI too to determine how much RAM we have */
            efi_memmap(NULL);
        }
        ram &= ~(2 * 1024 * 1024 - 1);
    }
}

/**
 * Parse config for modules and load them
 */
void fw_loadmodules(void)
{
    uint64_t unc_buf;
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
                    uncomp = 0;
                    if(tmp[0] == 0x1f && tmp[1] == 0x8b)
                        fw_read(file_size - 4, 4, (void*)&uncomp);
                    else
                    if(tmp[0] == 'G' && tmp[1] == 'U' && tmp[2] == 'D' && tmp[8] == 0x78)
                        uncomp = (((tmp[4] | (tmp[5] << 8)) + 7) & ~7) + ((tmp[6] | (tmp[7] << 8)) << 4);
                    if(ST) {
                        /* if it's a gzip compressed module, then get another buffer for uncompressed data,
                         * and then ditch the compressed buffer after inflation */
                        if(uncomp) unc_buf = efi_allocpages(AllocateAnyPages, (uncomp + 4095) >> 12, 0);
                        else unc_buf = 0;
                        mod_buf = efi_allocpages(AllocateAnyPages, (file_size + 4095) >> 12, 0);
                        ptr = unc_buf ? (uint8_t*)unc_buf : (uint8_t*)mod_buf;
                    } else {
                        ptr = (uint8_t*)file_buf;
                        /* if it's a gzip compressed module, then load it at position + uncompressed size,
                         * and uncompress to position. Compressed buffer will be overwritten by the next module. */
                        if(uncomp) {
                            unc_buf = file_buf;
                            file_buf += (uncomp + 4095) & ~4095;
                            mod_buf = file_buf;
                        } else {
                            unc_buf = 0;
                            mod_buf = file_buf;
                            file_buf += (file_size + 4095) & ~4095;
                        }
                    }
                    if(mod_buf) {
                        if(verbose) printf("Loading module '%S' (%ld bytes)...\r\n", wcname, file_size);
                        fw_read(0, file_size, (void*)mod_buf);
                        if(unc_buf) {
                            if(verbose) printf("Uncompressing (%d bytes)...\r\n", uncomp);
                            uncompress((uint8_t*)mod_buf, (uint8_t*)unc_buf, uncomp);
                        }
                        /* if it's a DTB, DSDT or a GUDT, don't add it to the modules list, add it to the ACPI tables */
                        if(ptr[0] == 0xD0 && ptr[1] == 0x0D && ptr[2] == 0xFE && ptr[3] == 0xED) {
                            if(verbose) printf("DTB detected...\r\n");
                            dsdt_ptr = (uint64_t)ptr;
                        } else
                        if(((ptr[0] == 'D' && ptr[1] == 'S') || (ptr[0] == 'G' && ptr[1] == 'U')) && ptr[2] == 'D' && ptr[3] == 'T') {
                            if(verbose) printf("%c%cDT detected...\n", ptr[0], ptr[1]);
                            dsdt_ptr = (uint64_t)ptr;
                        } else {
                            if(tags_ptr) {
                                tag = (multiboot_tag_module_t*)tags_ptr;
                                tag->type = MULTIBOOT_TAG_TYPE_MODULE;
                                tag->size = sizeof(multiboot_tag_module_t) + e - a + 1;
                                tag->mod_start = unc_buf ? (uint32_t)(uintptr_t)unc_buf : (uint32_t)(uintptr_t)mod_buf;
                                tag->mod_end = unc_buf ? (uint32_t)(uintptr_t)unc_buf + uncomp :
                                    (uint32_t)(uintptr_t)mod_buf + (uint32_t)file_size;
                                memcpy(tag->string, a, e - a); tag->string[e - a] = 0;
                                if(verbose > 2) printf("%D\r\n", tag->mod_start);
                                tags_ptr += (tag->size + 7) & ~7;
                                if(!initrd) initrd = tag;
                            }
                            n++;
                        }
                        if(ST && unc_buf && nptr) {
                            nptr--; BS->FreePages(ptrs[nptr].Memory, ptrs[nptr].NoPages);
                        }
                    }
                    fw_close();
                } else if(verbose) printf("WARNING: unable to load '%S'\r\n", wcname);
            }
            /* go to the next line */
            s = e;
        }
    }
    /* if no modules were loaded, but we have a default initrd name, try to add that */
    if(!n && !f) { f = 1; if((volatile char)definitrd[0]) { a = (char*)definitrd; for(e = a; *e; e++){} goto ldinitrd; } }
    if(!n && f == 1) { f = 2; a = bkp ? "ibmpc/initrd.bak" : "ibmpc/initrd"; e = a + (bkp ? 16 : 12); goto ldinitrd; }
}

/**
 * Map virtual memory
 */
int fw_map(uint64_t phys, uint64_t virt, uint32_t size)
{
    uint64_t end = virt + size, *ptr, *next = NULL, orig = file_buf;

    /* is this a canonical address? We handle virtual memory up to 256TB */
    if(!pt || ((virt >> 48L) != 0x0000 && (virt >> 48L) != 0xffff)) return 0;

    /* walk the page tables and add the missing pieces */
    for(virt &= ~4095, phys &= ~4095; virt < end; virt += 4096) {
        /* 512G */
        ptr = &pt[(virt >> 39L) & 511];
        if(!*ptr) { if(!(*ptr = (ST ? efi_alloc() : bios_alloc()))) return 0; else *ptr |= 3; }
        /* 1G */
        ptr = (uint64_t*)(*ptr & ~4095); ptr = &ptr[(virt >> 30L) & 511];
        if(!*ptr) { if(!(*ptr = (ST ? efi_alloc() : bios_alloc()))) return 0; else *ptr |= 3; }
        /* 2M if we previously had a large page here, split it into 4K pages */
        ptr = (uint64_t*)(*ptr & ~4095); ptr = &ptr[(virt >> 21L) & 511];
        if(!*ptr || *ptr & 0x80) { if(!(*ptr = (ST ? efi_alloc() : bios_alloc()))) return 0; else *ptr |= 3; }
        /* 4K */
        ptr = (uint64_t*)(*ptr & ~4095); ptr = &ptr[(virt >> 12L) & 511];
        /* if this page is already mapped, that means the kernel has invalid, overlapping segments */
        if(!*ptr) { *ptr = (uint64_t)next; next = ptr; }
    }
    /* resolve the linked list */
    for(end = ((phys == orig ? file_buf : phys) + size - 1) & ~4095; next; end -= 4096, next = ptr) {
        ptr = (uint64_t*)*next; *next = end | 3;
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
    if(verbose > 1) printf("  segment %08x[%08x] -> %08x[%08x]\r\n", offs, filesz, vaddr, memsz);
    size = (memsz + (vaddr & 4095) + 4095) & ~4095;
    if(ST) {
        if(vaddr > ram) {
            /* possibly a higher-half kernel's segment, we must map it */
            if(!(buf = (uint8_t*)efi_allocpages(AllocateAnyPages, size >> 12, 0)) || !fw_map((uint64_t)buf, vaddr, size)) goto err;
        } else {
            /* try to allocate memory exactly at the requested address */
            if(!efi_allocpages(AllocateAddress, size >> 12, vaddr & ~4095)) goto err;
            buf = (uint8_t*)(uintptr_t)vaddr;
        }
    } else {
        /* no overwriting of the loader data */
        if(vaddr < 0x20000 + (TAGS_MAX + 2) * 4096) goto err;
        if(vaddr > ram) {
            /* possibly a higher-half kernel's segment, we must map it */
            if(!fw_map(file_buf, vaddr, size)) goto err;
            buf = (void*)file_buf; file_buf += size;
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
    }
    if(filesz) fw_read(offs, filesz, buf);
    if(memsz > filesz) memset(buf + filesz, 0, memsz - filesz);
    return 1;
err:printf("ERROR: unable to load segment %08lx[%x], memory already in use\r\n", vaddr, memsz);
    return 0;
}

/**
 * Load the kernel
 */
int fw_loadkernel(void)
{
    void *p = (void*)kernel_buf;
    linux_boot_t *hdr = (linux_boot_t*)(kernel_buf + 0x1f1);
    efi_status_t status;
    pe_hdr *pe;
    pe_sec *sec;
    uint8_t *ptr;
    uint64_t offs;
    int i;

    /* make sure all previously allocated memory (in first pass) is freed, because we need fixed addresses */
    efi_freepages();
    wcname[0] = 0;
    if(!((kernel && *kernel && fw_open(kernel)) || fw_open("ibmpc/core"))) {
        smp = 0;
        if(wcname[0]) printf("ERROR: kernel '%S' not found\r\n", wcname);
        else printf("ERROR: kernel not found\r\n");
        /* if we are on BIOS and we have a partition marked bootable (or FreeBSD boot or BIOS boot partition), try that */
        if(!ST && vbr_lba && vbr_size) { kernel_mode = MODE_VBR; return 1; }
        return 0;
    }
    fw_read(0, sizeof(kernel_buf), p);
    /* we must check Linux before COFF/PE, because it might disguise itself as an EFI app */
    if(hdr->boot_flag == 0xAA55 && !memcmp(&hdr->header, HDRSMAG, 4)) {
        if(hdr->version < 0x20c || ((hdr->pref_address + file_size) >> 32L)) {
            printf("ERROR: unsupported Linux boot protocol version\r\n"); goto err;
        }
        /* it's a Linux kernel */
        kernel_mode = MODE_LIN; smp = 0;
        if(verbose) printf("Loading Linux kernel '%S'...\r\n", wcname);
        if(!zero_page) zero_page = (linux_boot_params_t*)(ST ? efi_alloc() : 0x90000);
        if(ST) {
            i = 0;
            if(cmdline) for(; cmdline[i] && cmdline[i] != '\r' && cmdline[i] != '\n'; i++);
            if(!zero_page) {
                i += (int)sizeof(linux_boot_params_t) + 1;
                status = BS->AllocatePages(AllocateMaxAddress, EfiLoaderData, (i + 4095) >> 12, (efi_physical_address_t*)&zero_page);
                if(EFI_ERROR(status)) {
                    zero_page = NULL;
                    status = BS->AllocatePages(AllocateAnyPages, EfiLoaderData, (i + 4095) >> 12, (efi_physical_address_t*)&zero_page);
                    if(EFI_ERROR(status) || ((uint64_t)zero_page >> 32L)) { printf("ERROR: zero page allocation error\r\n"); goto err; }
                }
            }
        }
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
        if(verbose) printf("Loading Multiboot2 ELF%d kernel '%S'...\r\n", kernel_mode == MODE_MB64 ? 64 : 32, wcname);
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
        kernel_mode = pe->file_type == PE_OPT_MAGIC_PE32PLUS ? MODE_MB64 : MODE_PE32;
        offs = kernel_mode == MODE_MB64 ? (uint32_t)pe->data.pe64.img_base : pe->data.pe32.img_base;
        kernel_entry = offs + (uint8_t*)(uintptr_t)pe->entry_point;
        if(verbose) printf("Loading Multiboot2 PE%d kernel '%S'...\r\n", kernel_mode == MODE_MB64 ? 64 : 32, wcname);
        sec = (pe_sec*)((uint8_t*)pe + pe->opt_hdr_size + 24);
        for(i = 0; !rq && i < pe->sections && (uint8_t*)&sec[1] < kernel_buf + sizeof(kernel_buf); i++, sec++)
            if(!fw_loadseg(sec->raddr, sec->rsiz,
                /* the PE section vaddr field is only 32 bits, we must make sure that it properly sign extended to 64 bit */
                offs + (pe->file_type == PE_OPT_MAGIC_PE32PLUS ? (int64_t)(int32_t)sec->vaddr : sec->vaddr), sec->vsiz)) goto err;
    } else {
        printf("ERROR: unknown kernel format '%S'\r\n", wcname);
err:    fw_close();
        smp = 0;
        return 0;
    }
    fw_close();
    /* force GRUB compatible prot mode entry point even with 64 bit kernels (volatile is needed because of a Clang optimizer bug) */
    if(kernel_mode == MODE_MB64 && (volatile char)definitrd[63] == 2) kernel_mode = MODE_MB32;
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
    multiboot_tag_mmap_t *tag;
    efi_status_t status;
    efi_memory_descriptor_t *memory_map = NULL;
    uintn_t memory_map_size = 0, map_key = 0, desc_size = 0;
    uint32_t desc_ver = 0, ow, oh, ob, a, b;
    uint8_t *rsdt = NULL, *lapic = NULL, *p, *q, *e, *ptr, *end, s, *edid = NULL;
    uint64_t c, d;
    static uint8_t ids[256];

    if(vidmode.framebuffer_addr) {
        if(vidmode.framebuffer_width != fb_w || vidmode.framebuffer_height != fb_h || vidmode.framebuffer_bpp != fb_bpp) {
            ow = vidmode.framebuffer_width; oh = vidmode.framebuffer_height; ob = vidmode.framebuffer_bpp;
            if(ST) efi_gop(fb_w, fb_h, fb_bpp); else fw_lfb(fb_w, fb_h, fb_bpp);
            if(!vidmode.framebuffer_addr) { if(ST) efi_gop(ow, oh, ob); else fw_lfb(ow, oh, ob); }
            fw_bootsplash();
        }
        if(tags_ptr && vidmode.framebuffer_addr) {
            vidmode.type = MULTIBOOT_TAG_TYPE_FRAMEBUFFER;
            vidmode.size = sizeof(vidmode);
            vidmode.framebuffer_type = 1;
            vidmode.reserved = 0;
            memcpy(tags_ptr, &vidmode, vidmode.size);
            tags_ptr += (vidmode.size + 7) & ~7;
        }
        fw_map(vidmode.framebuffer_addr, vidmode.framebuffer_addr,
            (vidmode.framebuffer_pitch * vidmode.framebuffer_height + 4095) & ~4095);
    }
    if(tags_ptr) {
        if(ST) efi_edid(&edid, (uint32_t*)&i); else { edid = FB ? (FB->video ? FB->video->edid : NULL) : (uint8_t*)0x580; i = 128; }
        if(edid && i > 0 && *((uint64_t*)(edid + 8))) {
            ((multiboot_tag_edid_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_EDID;
            ((multiboot_tag_edid_t*)tags_ptr)->size = i + 8;
            memcpy(tags_ptr + 8, edid, i);
            tags_ptr += (i + 15) & ~7;
        }
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
                    (uint8_t*)*((uint64_t*)&((multiboot_tag_new_acpi_t*)t)->rsdp[24]);
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
                        /* some buggy EFI (TianoCore *khm*) lists APIC table multiple times... */
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
            st->numcores = n; st->running = 1;
            __asm__ __volatile__ ("movl $1, %%eax; cpuid; shrl $24, %%ebx;" : "=b"(a) : : );
            st->bspid = a; *((uint64_t*)0x8fff8) = a;
            tags_ptr += (((multiboot_tag_smp_t*)tags_ptr)->size + 7) & ~7;
            /* measure CPU clock cycles (must do before ExitBootServices on EFI) */
            __asm__ __volatile__ ( "rdtsc" : "=a"(a),"=d"(b)); c = ((uint64_t)b << 32UL)|(uint64_t)a;
            ST ? BS->Stall(1000) : (FB ? FB->system->udelay(1000) : bios_sleep());
            __asm__ __volatile__ ( "rdtsc" : "=a"(a),"=d"(b));
            *((uint64_t*)0x548) = ((((uint64_t)b << 32UL)|(uint64_t)a) - c) / 5000;
            if(*((uint64_t*)0x548) < 1) *((uint64_t*)0x548) = 1;
        }
        /* partition UUIDs */
        ((multiboot_tag_partuuid_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_PARTUUID;
        ((multiboot_tag_partuuid_t*)tags_ptr)->size = 24;
        memcpy(((multiboot_tag_partuuid_t*)tags_ptr)->bootuuid, &bootuuid, sizeof(guid_t));
        tags_ptr += (((multiboot_tag_partuuid_t*)tags_ptr)->size + 7) & ~7;
        /* EFI tags */
        if(ST) {
            tag = (multiboot_tag_mmap_t*)tags_ptr;
            tag->type = MULTIBOOT_TAG_TYPE_MMAP;
            tag->entry_size = sizeof(multiboot_mmap_entry_t);
            tag->entry_version = 0;
            num_memmap = efi_memmap(tag->entries);
            if(num_memmap > 0) {
                memmap = tag->entries;
                tag->size = sizeof(multiboot_tag_mmap_t) + num_memmap * sizeof(multiboot_mmap_entry_t);
                tags_ptr += (tag->size + 7) & ~7;
            }
            ((multiboot_tag_efi64_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_EFI64;
            ((multiboot_tag_efi64_t*)tags_ptr)->size = 16;
            ((multiboot_tag_efi64_t*)tags_ptr)->pointer = (uintptr_t)ST;
            tags_ptr += (((multiboot_tag_efi64_t*)tags_ptr)->size + 7) & ~7;
            ((multiboot_tag_efi64_ih_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_EFI64_IH;
            ((multiboot_tag_efi64_ih_t*)tags_ptr)->size = 16;
            ((multiboot_tag_efi64_ih_t*)tags_ptr)->pointer = (uintptr_t)IM;
            tags_ptr += (((multiboot_tag_efi64_ih_t*)tags_ptr)->size + 7) & ~7;
        }
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
            zero_page->orig_video_isVGA = ST ? VIDEO_TYPE_EFI : VIDEO_TYPE_VLFB;
            zero_page->hdr.vid_mode = VIDEO_MODE_CUR;
        }
        zero_page->acpi_rsdp_addr = rsdp_ptr;
        if(initrd) {
            zero_page->hdr.ramdisk_image = initrd->mod_start;
            zero_page->hdr.ramdisk_size = initrd->mod_end - initrd->mod_start;
        }
    }
    if(ST) {
        efi_freeall();
        status = BS->GetMemoryMap(&memory_map_size, NULL, &map_key, &desc_size, NULL);
        /* if we're booting Linux, then pass UEFI data to zero page. Otherwise just simply use "map_key" for exit boot services */
        if(status == EFI_BUFFER_TOO_SMALL && kernel_mode == MODE_LIN && zero_page) {
            memory_map_size += 4 * desc_size;
            status = BS->AllocatePool(EfiLoaderData, memory_map_size, (void**)&memory_map);
            if(!EFI_ERROR(status)) {
                status = BS->GetMemoryMap(&memory_map_size, memory_map, &map_key, &desc_size, &desc_ver);
                if(!EFI_ERROR(status)) {
                    memcpy(&zero_page->efi_loader_signature, "EL64", 4);
                    zero_page->efi_memmap = (uint32_t)(uintptr_t)memory_map;
                    zero_page->efi_memmap_hi = (uint32_t)((uintptr_t)memory_map >> 32L);
                    zero_page->efi_memmap_size = memory_map_size;
                    zero_page->efi_memdesc_size = desc_size;
                    zero_page->efi_memdesc_version = desc_ver;
                    zero_page->efi_systab = (uint32_t)(uintptr_t)ST;
                    zero_page->efi_systab_hi = (uint32_t)((uintptr_t)ST >> 32L);
                }
            }
        }
        BS->ExitBootServices(IM, map_key);
    }
    /* new GDT (this must be below 1M because we need it in real mode) */
    *((uint16_t*)0x510) = 0x3F;                 /* value */
    *((uint64_t*)0x512) = 0x560;
    *((uint64_t*)0x568) = 0x000098000000FFFFUL; /*   8 - legacy real cs */
    *((uint64_t*)0x570) = 0x00CF9A000000FFFFUL; /*  16 - prot mode cs */
    *((uint64_t*)0x578) = 0x00CF92000000FFFFUL; /*  24 - prot mode ds */
    *((uint64_t*)0x580) = 0x00AF9A000000FFFFUL; /*  32 - long mode cs */
    *((uint64_t*)0x588) = 0x00CF92000000FFFFUL; /*  40 - long mode ds */
    *((uint64_t*)0x590) = 0x0000890000000068UL; /*  48 - long mode tss descriptor */
    *((uint64_t*)0x598) = 0x0000000000000000UL; /*       cont. */
    /* now that we have left the firmware realm behind, we can get some real work done :-) */
    __asm__ __volatile__ (
    /* fw_loadseg might have altered the paging tables for higher-half kernels. Better to reload */
    /* CR3 to kick the MMU, but on UEFI we can only do this after we have called ExitBootServices */
    "movq %%rax, %%cr3;"
    /* Set up dummy exception handlers */
    ".byte 0xe8;.long 0;"                       /* absolute address to set the code segment register */
    "1:popq %%rax;"
    "movq %%rax, %%rsi;addq $4f - 1b, %%rsi;"   /* pointer to the code stubs */
    "movq %%rax, %%rdi;addq $5f - 1b, %%rdi;"   /* pointer to IDT */
    "movq $0x598, %%rax;"                       /* patch GDT */
    "movq %%rax, %%rcx;andl $0xffffff, %%ecx;addl %%ecx, -6(%%rax);"
    "movq %%rax, %%rcx;shrq $24, %%rcx;movq %%rcx, -1(%%rax);"
    "lgdt (0x510);"                             /* we must set up a new GDT with a TSS */
    "movq $48, %%rax;ltr %%ax;"                 /* load TR */
    "movw $32, %%cx;\n"                         /* we set up 32 entires in IDT */
    "1:movq %%rsi, %%rax;movw $0x8F01, %%ax;shlq $16, %%rax;movw $32, %%ax;shlq $16, %%rax;movw %%si, %%ax;stosq;"
    "movq %%rsi, %%rax;shrq $32, %%rax;stosq;"
    "addq $16, %%rsi;decw %%cx;jnz 1b;"         /* next entry */
    "movw $2f-5f-1,(0x520);movq $5f, (0x522);"
    "lidt (0x520);jmp 2f;"                      /* set up IDT */
    /* TSS */
    ".long 0;.long 0x1000;.long 0;.long 0x1000;.long 0;.long 0x1000;.long 0;"
    ".long 0;.long 0;.long 0x1000;.long 0;"
    /* ISRs */
    "1:popq %%r8;movq 16(%%rsp),%%r9;jmp fw_exc;"
    ".balign 16;4:xorq %%rdx, %%rdx; xorb %%cl, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $1, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $2, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $3, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $4, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $5, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $6, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $7, %%cl;jmp 1b;"
    ".balign 16;popq %%rdx; movb $8, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $9, %%cl;jmp 1b;"
    ".balign 16;popq %%rdx; movb $10, %%cl;jmp 1b;"
    ".balign 16;popq %%rdx; movb $11, %%cl;jmp 1b;"
    ".balign 16;popq %%rdx; movb $12, %%cl;jmp 1b;"
    ".balign 16;popq %%rdx; movb $13, %%cl;jmp 1b;"
    ".balign 16;popq %%rdx; movb $14, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $15, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $16, %%cl;jmp 1b;"
    ".balign 16;popq %%rdx; movb $17, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $18, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $19, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $20, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $21, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $22, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $23, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $24, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $25, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $26, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $27, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $28, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $29, %%cl;jmp 1b;"
    ".balign 16;popq %%rdx; movb $30, %%cl;jmp 1b;"
    ".balign 16;xorq %%rdx, %%rdx; movb $31, %%cl;jmp 1b;"
    /* IDT */
    ".balign 16;5:.space (32*16);2:"
    ::"a"(pt):"rcx","rsi","rdi");
    if(smp && n > 1 && lapic && st) {
/* Memory layout (only valid when kernel entry isn't zero)
 *    0x510 -   0x520   GDT value
 *    0x520 -   0x530   IDT value
 *    0x530 -   0x538   page table root
 *    0x538 -   0x540   kernel entry point (also SMP semaphor)
 *    0x540 -   0x548   tags_buf
 *    0x548 -   0x550   CPU clockcycles in 1 msec
 *    0x550 -   0x558   lapic address
 *    0x558 -   0x559   AP is running flag
 *    0x560 -   0x590   GDT table
 */
        if(verbose) printf("Initializing SMP (%d cores)...\n", n);
        *((volatile uint64_t*)0x530) = (uint64_t)pt;
        *((volatile uint64_t*)0x538) = (uint64_t)0;
        *((volatile uint64_t*)0x540) = (uintptr_t)tags_buf;
        *((volatile uint64_t*)0x550) = (uint64_t)lapic;
        /* relocate AP startup code to 0x8000 (this will destroy getint(), gethex(), but that's
         * okay, we have already finished parsing the configuration file, we don't need 'em) */
        __asm__ __volatile__(
        /* relocate code */
        ".byte 0xe8;.long 0;"
        "1:popq %%rsi;addq $1f - 1b, %%rsi;movq $0x8000, %%rdi;movq $99f - 1f, %%rcx;repnz movsb;jmp 99f;"
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
        "lidt (0x520);movq (0x550), %%rbx;"
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
        "99:":::);

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
    *((uint64_t*)0x558) = 0;
}

/**
 * Dummy exception handler
 */
void fw_exc(uint8_t excno, uint64_t exccode, uint64_t rip, uint64_t rsp)
{
    uint64_t cr2, cr3;
#if defined(CONSOLE_FB) || defined(CONSOLE_VGA)
    uint32_t i;
#endif
#ifdef CONSOLE_FB
    uint32_t j, x, y, b;
#endif
    if(!in_exc) {
        in_exc++;
        __asm__ __volatile__("movq %%cr2, %%rax;movq %%cr3, %%rbx;":"=a"(cr2),"=b"(cr3)::);
#ifdef CONSOLE_FB
        if(vidmode.framebuffer_addr) {
            b = (vidmode.framebuffer_bpp + 7) >> 3;
            fb_x = fb_y = 4; fb_bg = FB_COLOR(255, 0, 0);
            for(j = y = 0; y < 8 + (((psf2_t*)font_psf)->height << 1); y++, j += vidmode.framebuffer_pitch)
                for(i = j, x = 0; x < vidmode.framebuffer_width; x++, i += b)
                    if(b == 2) *((uint16_t*)(vidmode.framebuffer_addr + i)) = (uint16_t)fb_bg;
                    else *((uint32_t*)(vidmode.framebuffer_addr + i)) = fb_bg;
        }
#endif
#ifdef CONSOLE_VGA
        vga_x = vga_y = 0;
        if(!vidmode.framebuffer_addr && !ST) {
            for(i = 0; i < 320; i += 2) *((uint16_t*)((uintptr_t)0xB8000 + i)) = 0x4f20;
            for(; i < 2000; i += 2) *((uint16_t*)((uintptr_t)0xB8000 + i)) = 0x0f20;
        }
#endif
        printf("Simpleboot Exception Handler\r\nException %02x #%s code %016x\r\n\r\n",
            excno, excno < 32 ? excstr[(int)excno] : "INTR", exccode);
#ifdef CONSOLE_FB
        fb_bg = 0;
#endif
        printf("RIP %016x RSP %016x CR2 %016x CR3 %016x\r\n\r\nCode\r\n%D\r\nStack\r\n%4D\r\n",
            rip, rsp, cr2, cr3, rip, rsp);
    }
    __asm__ __volatile__("1: cli; hlt; jmp 1b");
}

/*****************************************
 *     Simpleboot loader entry point     *
 *****************************************/
efi_status_t _start (efi_handle_t image, efi_system_table_t *systab, uint16_t bdev)
{
    uint8_t al;

    /* initialize UEFI or BIOS */
    fw_init(image, systab, bdev);
    printf("Simpleboot loader, Copyright (c) 2023 bzt, MIT license\r\n");
    /* now that we can display error messages, let's see if we got everything we need */
    if(!pt) { printf("ERROR: unable to allocate memory\r\n"); goto err; }
    if(!root_dir) { printf("ERROR: unable to locate boot partition\r\n"); goto err; }

    /* load and parse simpleboot.cfg */
again:
    fw_loadconfig();
    fw_loadsetup();
    if(!ram) { printf("ERROR: unable to determine the amount of RAM\r\n"); goto err; }
    else if(verbose && !bkp) printf("Physical RAM %ld Megabytes\r\n", ram / 1024 / 1024 + 2);

    /* now we have the kernel's filename, try to load that, it's a critical error if fails */
    if(!fw_loadkernel()) goto err;
    /* last step, load modules too */
    fw_loadmodules();
    /* if the user pressed a key during loading, fallback to backup and do over */
    if(!bkp && rq) { bkp++; rq = 0; goto again; }

    /* finish up things, finalize tags list */
    fw_fini();

    /* transfer control to kernel. Should never return */
    if(kernel_mode != MODE_VBR) {
        if(!kernel_entry) { printf("ERROR: no kernel entry point\r\n"); goto err; }
        if(verbose > 2) printf("Kernel entry:\r\n%4D", kernel_entry);
    }
    switch(kernel_mode) {
        case MODE_VBR:
            if(verbose > 1)
                printf("Transfering real mode control to 0:7C00 (LBA %d size %d sector(s))\r\n", vbr_lba, vbr_size);
            *((uint16_t*)0x502) = vbr_size > 127 ? 127 : vbr_size; *((uint32_t*)0x504) = 0x7C00; *((uint64_t*)0x508) = vbr_lba;
            bios_fallback();
        break;
        case MODE_PE32:
        case MODE_MB32:
            if(verbose > 1)
                printf("Transfering prot mode control to %08x(%08x, %08x[%x])\r\n", kernel_entry,
                    MULTIBOOT2_BOOTLOADER_MAGIC, tags_buf, tags_ptr - tags_buf);
            /* execute 32-bit kernels in protected mode */
            *((uint32_t*)0x8fffc) = (uint32_t)(uintptr_t)tags_buf;
            *((uint32_t*)0x8fff8) = MULTIBOOT2_BOOTLOADER_MAGIC;
            *((uint32_t*)0x8fff4) = 0xDEADBEEF;
            __asm__ __volatile__(
            "movq $0x10, %%rax;push %%rax;"     /* unfortunately Clang doesn't allow "callq 1f(%%rip)", */
            "movq $0x600, %%rdi;push %%rdi;"    /* but we need to relocate trampoline to 0x600 (we need */
            ".byte 0xe8;.long 0;"               /* absolute address to set the code segment register) */
            "1:popq %%rsi;"
            "addq $1f - 1b, %%rsi;"
            "movq $2f - 1f, %%rcx;"
            "repnz movsb;"
            "lretq;.code32;1:;"                 /* long -> compat, setting code segment to 32-bit */
            "movl %%cr0, %%eax;"                /* disable paging */
            "btcl $31, %%eax;"
            "movl %%eax, %%cr0;"
            "movw $0x18, %%ax;movw %%ax, %%ds;" /* set 32-bit segment selectors */
            "movw %%ax, %%es;movw %%ax, %%ss;"
            "movw %%ax, %%fs;movw %%ax, %%gs;"
            "movl %%ebx, %%esi;"
            "movl $0x0C0000080, %%ecx;"         /* disable long in EFER MSR */
            "rdmsr;btcl $8, %%eax;wrmsr;"       /* on 32-bit, this messes with %edx */
            "xorl %%eax, %%eax;lidt (%%eax);"   /* disable IDT */
            /* CDECL uses the stack for arguments, but fastcall uses %ecx, %edx */
            "movl $0x8fff4, %%esp; movl %%esp, %%ebp;"
            "movl 8(%%esp), %%edx;movl %%edx, %%ebx;movl 4(%%esp), %%eax; movl %%eax, %%ecx;"
            "jmp *%%esi;2:;"
            ::"b"(kernel_entry):);
        break;
        case MODE_MB64:
            if(verbose > 1)
                printf("Transfering long mode control to %08x(%08x, %08x[%x])\r\n", kernel_entry,
                    MULTIBOOT2_BOOTLOADER_MAGIC, tags_buf, tags_ptr - tags_buf);
            /* tell APs to execute kernel */
            if(smp) { *((volatile uint64_t*)0x538) = (uintptr_t)kernel_entry; __asm__ __volatile__("pause":::"memory"); }
            /* execute 64-bit kernels in long mode */
             __asm__ __volatile__(
            "movq %%rcx, %%r8;"
            /* SysV ABI uses %rdi, %rsi, but fastcall uses %rcx, %rdx */
            "movq %%rax, %%rcx;movq %%rax, %%rdi;"
            "movq %%rbx, %%rdx;movq %%rbx, %%rsi;"
            "movq $0x90000, %%rsp; movq %%rsp, %%rbp; subl $8, %%esp;"
            "jmp *%%r8"
            ::"a"(MULTIBOOT2_BOOTLOADER_MAGIC),"b"(tags_buf),"c"(kernel_entry):);
        break;
        case MODE_LIN:
            if(verbose > 1)
                printf("Transfering long mode control to %08x(%08x)\r\n", kernel_entry, zero_page);
            /* execute Linux kernel in 64 bit mode */
            __asm__ __volatile__(
            "jmp *%%rax"
            ::"S"(zero_page),"a"(kernel_entry):);
        break;
    }
    printf("ERROR: kernel should not have returned\r\n");

    /* there's nowhere to return to on BIOS, halt machine */
err:if(!systab) {
        if(bkp) __asm__ __volatile__("1: cli; hlt; jmp 1b");
        else while(1) {
            __asm__ __volatile__("inb $0x64, %%al;pause;pause;pause;":"=a"(al)::);
            if(al != 0xFF && al & 1) { bkp++; rq = 0; goto again; }
        }
    }
    /* on UEFI we should return an error status, and the firmware should get the next option from the boot order list */
    return EFI_LOAD_ERROR;
}
