/*
 * src/loader_rpi.c
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
 * @brief The main Simpleboot loader program on Raspberry Pi
 *
 * Memory layout when booted on Raspberry Pi:
 *      0x0 -    0x400  reserved by firmware
 *    0x400 -   0x1000  stack (3072 bytes)
 *   0x1000 -  0x20000  paging tables
 *  0x20000 -  0x80000  config + logo + tags; from the top to bottom: kernel's stack
 *  0x80000 - _bss_end  our sections
 * _bss_end -        x  kernel segments, followed by the modules, each page aligned
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
#define CONSOLE_SERIAL                              /* default serial, UART0 */
#define CONSOLE_FB                                  /* on screen too */

/* it is VERY important that these two variables must be the first in the
 * read-only data segment, because the disk generator might alter them */
const char __attribute__((aligned(16))) defkernel[64] = "kernel", definitrd[64] = "";
/* patched by bin2h.c, must be right after the default names. Must use volatile otherwise Clang optimizes memread away */
const volatile unsigned int _bss_start = 0xffffffff, _bss_end = 0xffffffff;

/**
 * Assembly preambule to C
 * must be the very first function; it can't use any local variables and can't have a stack frame
 */
void __attribute__((noreturn)) /*__attribute__((naked))*/ _preambule(void)
{
    /* make sure we run on BSP only, set up EL1 and a stack */
    __asm__ __volatile__(
    "movk x0,#0,lsl #32;adr x1,dtb_base;str x0,[x1];"/* save device tree base */
    "mrs x1, mpidr_el1;and x1, x1, #3;cbz x1, 2f;"  /* get core id and branch if BSP */
    "1:wfe; b 1b;"                                  /* neverending loop for APs */
    ".balign 64;.asciz \"Simpleboot https://codeberg.org/bzt/simpleboot\";.asciz \"aarch64\";.balign 4;"
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
    "1:adr x0,1f;msr vbar_el1,x0;msr SPSel,#0;"                                         /* set up exception handlers */
    "mov sp, x1;b _start;"                        /* set up stack and jump to C function in EL1 */
    /* exception handlers */
    ".balign 128;1:mov x0,#0;mrs x1,esr_el1;mrs x2,elr_el1;mrs x3,spsr_el1;mrs x4,far_el1;mrs x5,sctlr_el1;mrs x6,tcr_el1;b fw_exc;"
    ".balign 128;mov x0,#1;mrs x1,esr_el1;mrs x2,elr_el1;mrs x3,spsr_el1;mrs x4,far_el1;mrs x5,sctlr_el1;mrs x6,tcr_el1;b fw_exc;"
    ".balign 128;mov x0,#2;mrs x1,esr_el1;mrs x2,elr_el1;mrs x3,spsr_el1;mrs x4,far_el1;mrs x5,sctlr_el1;mrs x6,tcr_el1;b fw_exc;"
    ".balign 128;mov x0,#3;mrs x1,esr_el1;mrs x2,elr_el1;mrs x3,spsr_el1;mrs x4,far_el1;mrs x5,sctlr_el1;mrs x6,tcr_el1;b fw_exc;"
    :::"x0","x1","x2");
    /* naked not supported, so noreturn and unreachable needed to make the compiler omit the stack frame prologue / epilogue */
    __builtin_unreachable();
}

#include "../simpleboot.h"
#include "loader.h"

/* IMPORTANT: don't assume .bss is zeroed out like in a hosted environment, because it's not */
volatile uint32_t  __attribute__((aligned(16))) mbox[40];
uint8_t __attribute__((aligned(16))) vbr[512], data[512];
uint32_t fat[1024], fat_cache, file_clu;
uint16_t lfn[272], wcname[PATH_MAX];
uint64_t mmio_base, emmc_base;
uint64_t file_size, rsdp_ptr, file_base, file_buf, mod_buf, ram, *pt;
uint32_t fb_w, fb_h, fb_bpp, fb_bg, logo_size, verbose, num_memmap, pb_b, pb_m, pb_l, rq, bkp;
uint8_t rpi, *tags_buf, *tags_ptr, *logo_buf, *dtb_base, *kernel_entry, kernel_mode, kernel_buf[4096], *pb_fb, in_exc, smp;
uint64_t root_dir, data_lba, fat_lba;
guid_t espGuid = EFI_PART_TYPE_EFI_SYSTEM_PART_GUID, bootuuid;
esp_bpb_t *bpb;
multiboot_tag_framebuffer_t vidmode;
multiboot_tag_module_t *initrd;
char *conf_buf, *kernel, *cmdline;

apic_t __attribute__((aligned(16))) apic;
rsdt_t __attribute__((aligned(16))) rsdt;
rsdp_t __attribute__((aligned(16))) rsdp;
fadt_t __attribute__((aligned(16))) fadt;

/**************** Mandatory functions, Clang generates calls to them ****************/

void memcpy(void *dst, const void *src, uint32_t n){uint8_t *a=(uint8_t*)dst,*b=(uint8_t*)src;while(n--) *a++=*b++; }
void memset(void *dst, uint8_t c, uint32_t n){uint8_t *a=dst;while(n--) *a++=c; }
int  memcmp(const void *s1, const void *s2, uint32_t n){
    uint8_t *a=(uint8_t*)s1,*b=(uint8_t*)s2;while(n--){if(*a!=*b){return *a-*b;}a++;b++;} return 0;
}

#include "inflate.h"

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

/**************** GPU MailBox interface ****************/

#define VIDEOCORE_MBOX  (mmio_base+0x0000B880)
#define MBOX_READ       ((volatile uint32_t*)(VIDEOCORE_MBOX+0x0))
#define MBOX_POLL       ((volatile uint32_t*)(VIDEOCORE_MBOX+0x10))
#define MBOX_SENDER     ((volatile uint32_t*)(VIDEOCORE_MBOX+0x14))
#define MBOX_STATUS     ((volatile uint32_t*)(VIDEOCORE_MBOX+0x18))
#define MBOX_CONFIG     ((volatile uint32_t*)(VIDEOCORE_MBOX+0x1C))
#define MBOX_WRITE      ((volatile uint32_t*)(VIDEOCORE_MBOX+0x20))
#define MBOX_REQUEST    0
#define MBOX_RESPONSE   0x80000000
#define MBOX_FULL       0x80000000
#define MBOX_EMPTY      0x40000000
#define MBOX_CH_POWER   0
#define MBOX_CH_FB      1
#define MBOX_CH_VUART   2
#define MBOX_CH_VCHIQ   3
#define MBOX_CH_LEDS    4
#define MBOX_CH_BTNS    5
#define MBOX_CH_TOUCH   6
#define MBOX_CH_COUNT   7
#define MBOX_CH_PROP    8

uint8_t mbox_call(uint8_t ch)
{
    uint32_t r;

    /* mailbox write */
    do{ __asm__ __volatile__("nop"); } while(*MBOX_STATUS & MBOX_FULL);
    *MBOX_WRITE = (((uint32_t)((uint64_t)mbox) & ~0xF) | (ch & 0xF));
    /* mailbox read */
    do {
        do{ __asm__ __volatile__("nop");} while(*MBOX_STATUS & MBOX_EMPTY);
        r = *MBOX_READ;
    } while((uint8_t)(r & 0xF) != ch);
    return (r & ~0xF) == (uint32_t)((uint64_t)mbox) && mbox[1] == MBOX_RESPONSE;
}

/**
 * Set up framebuffer with VideoCore
 */
void mbox_lfb(uint32_t width, uint32_t height, uint32_t bpp)
{
    /* query natural width, height if not given */
    if(!width || !height) {
        mbox[0] = 12*4;
        mbox[1] = MBOX_REQUEST;
        mbox[2] = 0x40003;  /* get phy wh */
        mbox[3] = 8;
        mbox[4] = 8;
        mbox[5] = 0;
        mbox[6] = 0;
        mbox[7] = 0x40005;  /* get depth */
        mbox[8] = 4;
        mbox[9] = 4;
        mbox[10] = 0;
        mbox[11] = 0;
        if(mbox_call(MBOX_CH_PROP) && mbox[5] && mbox[10]) {
            width = mbox[5];
            height = mbox[6];
            if(width < 800) width = 800;
            if(height < 600) height = 600;
            bpp = 32;
        }
    }
    /* if we already have a framebuffer, release it */
    if(vidmode.framebuffer_addr) {
        mbox[0] = 8*4;
        mbox[1] = MBOX_REQUEST;
        mbox[2] = 0x48001;  /* release buffer */
        mbox[3] = 8;
        mbox[4] = 8;
        mbox[5] = (uint32_t)vidmode.framebuffer_addr;
        mbox[6] = 0;
        mbox[7] = 0;
        mbox_call(MBOX_CH_PROP);
        vidmode.framebuffer_addr = 0;
    }
    /* check minimum resolution */
    if(width < 320) width = 320;
    if(height < 200) height = 200;
    if(bpp != 15 && bpp != 16 && bpp != 24) bpp = 32;

    mbox[0] = 35*4;
    mbox[1] = MBOX_REQUEST;

    mbox[2] = 0x48003;  /* set phy wh */
    mbox[3] = 8;
    mbox[4] = 8;
    mbox[5] = width;
    mbox[6] = height;

    mbox[7] = 0x48004;  /* set virt wh */
    mbox[8] = 8;
    mbox[9] = 8;
    mbox[10] = width;
    mbox[11] = height;

    mbox[12] = 0x48009; /* set virt offset */
    mbox[13] = 8;
    mbox[14] = 8;
    mbox[15] = 0;
    mbox[16] = 0;

    mbox[17] = 0x48005; /* set depth */
    mbox[18] = 4;
    mbox[19] = 4;
    mbox[20] = bpp;

    mbox[21] = 0x48006; /* set pixel order */
    mbox[22] = 4;
    mbox[23] = 4;
    mbox[24] = 1;       /* RGB, not BGR preferably */

    mbox[25] = 0x40001; /* get framebuffer, gets alignment on request */
    mbox[26] = 8;
    mbox[27] = 8;
    mbox[28] = 4096;
    mbox[29] = 0;

    mbox[30] = 0x40008; /* get pitch */
    mbox[31] = 4;
    mbox[32] = 4;
    mbox[33] = 0;

    mbox[34] = 0;

    if(mbox_call(MBOX_CH_PROP) && mbox[20] == bpp && mbox[27] == (MBOX_RESPONSE|8) && mbox[28]) {
        vidmode.framebuffer_addr = (uint64_t)(mbox[28] & 0x3FFFFFFF);
        vidmode.framebuffer_pitch = mbox[33];
        vidmode.framebuffer_width = mbox[5];
        vidmode.framebuffer_height = mbox[6];
        vidmode.framebuffer_bpp = mbox[20];
        vidmode.framebuffer_type = 1;
        if(mbox[24]) {
            /* red is the least significant channel */
            switch(mbox[20]) {
                case 15:
                    vidmode.framebuffer_red_mask_size = vidmode.framebuffer_green_mask_size =
                        vidmode.framebuffer_blue_mask_size = 5;
                    vidmode.framebuffer_red_field_position = 0;
                    vidmode.framebuffer_green_field_position = 5;
                    vidmode.framebuffer_blue_field_position = 10;
                break;
                case 16:
                    vidmode.framebuffer_red_mask_size = vidmode.framebuffer_blue_mask_size = 5;
                    vidmode.framebuffer_green_mask_size = 6;
                    vidmode.framebuffer_red_field_position = 0;
                    vidmode.framebuffer_green_field_position = 5;
                    vidmode.framebuffer_blue_field_position = 11;
                break;
                default:
                    vidmode.framebuffer_red_mask_size = vidmode.framebuffer_green_mask_size =
                        vidmode.framebuffer_blue_mask_size = 8;
                    vidmode.framebuffer_red_field_position = 0;
                    vidmode.framebuffer_green_field_position = 8;
                    vidmode.framebuffer_blue_field_position = 16;
                break;
            }
        } else {
            /* blue is the least significant channel */
            switch(mbox[20]) {
                case 15:
                    vidmode.framebuffer_red_mask_size = vidmode.framebuffer_green_mask_size =
                        vidmode.framebuffer_blue_mask_size = 5;
                    vidmode.framebuffer_red_field_position = 10;
                    vidmode.framebuffer_green_field_position = 5;
                    vidmode.framebuffer_blue_field_position = 0;
                break;
                case 16:
                    vidmode.framebuffer_red_mask_size = vidmode.framebuffer_blue_mask_size = 5;
                    vidmode.framebuffer_green_mask_size = 6;
                    vidmode.framebuffer_red_field_position = 11;
                    vidmode.framebuffer_green_field_position = 5;
                    vidmode.framebuffer_blue_field_position = 0;
                break;
                default:
                    vidmode.framebuffer_red_mask_size = vidmode.framebuffer_green_mask_size =
                        vidmode.framebuffer_blue_mask_size = 8;
                    vidmode.framebuffer_red_field_position = 16;
                    vidmode.framebuffer_green_field_position = 8;
                    vidmode.framebuffer_blue_field_position = 0;
                break;
            }
        }
    } else {
        if(vidmode.framebuffer_addr) {
            mbox[0] = 8*4;
            mbox[1] = MBOX_REQUEST;
            mbox[2] = 0x48001;  /* release buffer */
            mbox[3] = 8;
            mbox[4] = 8;
            mbox[5] = (uint32_t)vidmode.framebuffer_addr;
            mbox[6] = 0;
            mbox[7] = 0;
            mbox_call(MBOX_CH_PROP);
            vidmode.framebuffer_addr = 0;
        }
        vidmode.framebuffer_width = vidmode.framebuffer_height = vidmode.framebuffer_bpp = 0;
    }
}

/**************** Early boot console ****************/

#define GPFSEL0         ((volatile uint32_t*)(mmio_base+0x00200000))
#define GPFSEL1         ((volatile uint32_t*)(mmio_base+0x00200004))
#define GPFSEL2         ((volatile uint32_t*)(mmio_base+0x00200008))
#define GPFSEL3         ((volatile uint32_t*)(mmio_base+0x0020000C))
#define GPFSEL4         ((volatile uint32_t*)(mmio_base+0x00200010))
#define GPFSEL5         ((volatile uint32_t*)(mmio_base+0x00200014))
#define GPSET0          ((volatile uint32_t*)(mmio_base+0x0020001C))
#define GPSET1          ((volatile uint32_t*)(mmio_base+0x00200020))
#define GPCLR0          ((volatile uint32_t*)(mmio_base+0x00200028))
#define GPLEV0          ((volatile uint32_t*)(mmio_base+0x00200034))
#define GPLEV1          ((volatile uint32_t*)(mmio_base+0x00200038))
#define GPEDS0          ((volatile uint32_t*)(mmio_base+0x00200040))
#define GPEDS1          ((volatile uint32_t*)(mmio_base+0x00200044))
#define GPHEN0          ((volatile uint32_t*)(mmio_base+0x00200064))
#define GPHEN1          ((volatile uint32_t*)(mmio_base+0x00200068))
#define GPPUD           ((volatile uint32_t*)(mmio_base+0x00200094))
#define GPPUDCLK0       ((volatile uint32_t*)(mmio_base+0x00200098))
#define GPPUDCLK1       ((volatile uint32_t*)(mmio_base+0x0020009C))

#define UART0_DR        ((volatile uint32_t*)(mmio_base+0x00201000))
#define UART0_FR        ((volatile uint32_t*)(mmio_base+0x00201018))
#define UART0_IBRD      ((volatile uint32_t*)(mmio_base+0x00201024))
#define UART0_FBRD      ((volatile uint32_t*)(mmio_base+0x00201028))
#define UART0_LCRH      ((volatile uint32_t*)(mmio_base+0x0020102C))
#define UART0_CR        ((volatile uint32_t*)(mmio_base+0x00201030))
#define UART0_IMSC      ((volatile uint32_t*)(mmio_base+0x00201038))
#define UART0_ICR       ((volatile uint32_t*)(mmio_base+0x00201044))

#ifdef CONSOLE_FB
typedef struct { uint32_t magic, version, headersize, flags, numglyph, bytesperglyph, height, width; } __attribute__((packed)) psf2_t;
uint8_t font_psf[2080] = { 114,181,74,134,0,0,0,0,32,0,0,0,0,0,12,0,128,0,0,0,16,0,0,0,16,0,0,0,8,0,0,0,0,0,218,2,128,130,2,128,130,2,128,182,0,0,0,0,0,0,126,129,165,129,129,189,153,129,129,126,0,0,0,0,0,0,126,255,219,255,255,195,231,255,255,126,0,0,0,0,0,0,0,0,108,254,254,254,254,124,56,16,0,0,0,0,0,0,0,0,16,56,124,254,124,56,16,0,0,0,0,0,0,0,0,24,60,60,231,231,231,24,24,60,0,0,0,0,0,0,0,24,60,126,255,255,126,24,24,60,0,0,0,0,0,0,0,0,0,0,24,60,60,24,0,0,0,0,0,0,255,255,255,255,255,255,231,195,195,231,255,255,255,255,255,255,0,0,0,0,0,60,102,66,66,102,60,0,0,0,0,0,255,255,255,255,255,195,153,189,189,153,195,255,255,255,255,255,0,0,30,14,26,50,120,204,204,204,204,120,0,0,0,0,0,0,60,102,102,102,102,60,24,126,24,24,0,0,0,0,0,0,63,51,63,48,48,48,48,112,240,224,0,0,0,0,0,0,127,99,127,99,99,99,99,103,231,230,192,0,0,0,0,0,0,24,24,219,60,231,60,219,24,24,0,0,0,0,0,128,192,224,240,248,254,248,240,224,192,128,0,0,0,0,0,2,6,14,30,62,254,62,30,14,6,2,0,0,0,0,0,0,24,60,126,24,24,24,126,60,24,0,0,0,0,0,0,0,102,102,102,102,102,102,102,0,102,102,0,0,0,0,0,0,127,219,219,219,123,27,27,27,27,27,0,0,0,0,0,124,198,96,56,108,198,198,108,56,12,198,124,0,0,0,0,0,0,0,0,0,0,0,254,254,254,254,0,0,0,0,0,0,24,60,126,24,24,24,126,60,24,126,0,0,0,0,0,0,24,60,126,24,24,24,24,24,24,24,0,0,0,0,0,0,24,24,24,24,24,24,24,126,60,24,0,0,0,0,0,0,0,0,0,24,12,254,12,24,0,0,0,0,0,0,0,0,0,0,0,48,96,254,96,48,0,0,0,0,0,0,0,0,0,0,0,0,192,192,192,254,0,0,0,0,0,0,0,0,0,0,0,40,108,254,108,40,0,0,0,0,0,0,0,0,0,0,16,56,56,124,124,254,254,0,0,0,0,0,0,0,0,0,254,254,124,124,56,56,16,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,24,60,60,60,24,24,24,0,24,24,0,0,0,0,0,102,102,102,36,0,0,0,0,0,0,0,0,0,0,0,0,0,0,108,108,254,108,108,108,254,108,108,0,0,0,0,24,24,124,198,194,192,124,6,6,134,198,124,24,24,0,0,0,0,0,0,194,198,12,24,48,96,198,134,0,0,0,0,0,0,56,108,108,56,118,220,204,204,204,118,0,0,0,0,0,48,48,48,32,0,0,0,0,0,0,0,0,0,0,0,0,0,12,24,48,48,48,48,48,48,24,12,0,0,0,0,0,0,48,24,12,12,12,12,12,12,24,48,0,0,0,0,0,0,0,0,0,102,60,255,60,102,0,0,0,0,0,0,0,0,0,0,0,24,24,126,24,24,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,24,24,24,48,0,0,0,0,0,0,0,0,0,0,254,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,24,24,0,0,0,0,0,0,0,0,2,6,12,24,48,96,192,128,0,0,0,0,0,0,56,108,198,198,214,214,198,198,108,56,0,0,0,0,0,0,24,56,120,24,24,24,24,24,24,126,0,0,0,0,0,0,124,198,6,12,24,48,96,192,198,254,0,0,0,0,0,0,124,198,6,6,60,6,6,6,198,124,0,0,0,0,0,0,12,28,60,108,204,254,12,12,12,30,0,0,0,0,0,0,254,192,192,192,252,6,6,6,198,124,0,0,0,0,0,0,56,96,192,192,252,198,198,198,198,124,0,0,0,0,0,0,254,198,6,6,12,24,48,48,48,48,0,0,0,0,0,0,124,198,198,198,124,198,198,198,198,124,0,0,0,0,0,0,124,198,198,198,126,6,6,6,12,120,0,0,0,0,0,0,0,0,24,24,0,0,0,24,24,0,0,0,0,0,0,0,0,0,24,24,0,0,0,24,24,48,0,0,0,0,0,0,0,6,12,24,48,96,48,24,12,6,0,0,0,0,0,0,0,0,0,126,0,0,126,0,0,0,0,0,0,0,0,0,0,96,48,24,12,6,12,24,48,96,0,0,0,0,0,0,124,198,198,12,24,24,24,0,24,24,0,0,0,0,0,0,0,124,198,198,222,222,222,220,192,124,0,0,0,0,0,0,16,56,108,198,198,254,198,198,198,198,0,0,0,0,0,0,252,102,102,102,124,102,102,102,102,252,0,0,0,0,0,0,60,102,194,192,192,192,192,194,102,60,0,0,0,0,0,0,248,108,102,102,102,102,102,102,108,248,0,0,0,0,0,0,254,102,98,104,120,104,96,98,102,254,0,0,0,0,0,0,254,102,98,104,120,104,96,96,96,240,0,0,0,0,0,0,60,102,194,192,192,222,198,198,102,58,0,0,0,0,0,0,198,198,198,198,254,198,198,198,198,198,0,0,0,0,0,0,60,24,24,24,24,24,24,24,24,60,0,0,0,0,0,0,30,12,12,12,12,12,204,204,204,120,0,0,0,0,0,0,230,102,102,108,120,120,108,102,102,230,0,0,0,0,0,0,240,96,96,96,96,96,96,98,102,254,0,0,0,0,0,0,198,238,254,254,214,198,198,198,198,198,0,0,0,0,0,0,198,230,246,254,222,206,198,198,198,198,0,0,0,0,0,0,124,198,198,198,198,198,198,198,198,124,0,0,0,0,0,0,252,102,102,102,124,96,96,96,96,240,0,0,0,0,0,0,124,198,198,198,198,198,198,214,222,124,12,14,0,0,0,0,252,102,102,102,124,108,102,102,102,230,0,0,0,0,0,0,124,198,198,96,56,12,6,198,198,124,0,0,0,0,0,0,126,126,90,24,24,24,24,24,24,60,0,0,0,0,0,0,198,198,198,198,198,198,198,198,198,124,0,0,0,0,0,0,198,198,198,198,198,198,198,108,56,16,0,0,0,0,0,0,198,198,198,198,214,214,214,254,238,108,0,0,0,0,0,0,198,198,108,124,56,56,124,108,198,198,0,0,0,0,0,0,102,102,102,102,60,24,24,24,24,60,0,0,0,0,0,0,254,198,134,12,24,48,96,194,198,254,0,0,0,0,0,0,60,48,48,48,48,48,48,48,48,60,0,0,0,0,0,0,0,128,192,224,112,56,28,14,6,2,0,0,0,0,0,0,60,12,12,12,12,12,12,12,12,60,0,0,0,0,16,56,108,198,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,255,0,0,48,48,24,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,120,12,124,204,204,204,118,0,0,0,0,0,0,224,96,96,120,108,102,102,102,102,124,0,0,0,0,0,0,0,0,0,124,198,192,192,192,198,124,0,0,0,0,0,0,28,12,12,60,108,204,204,204,204,118,0,0,0,0,0,0,0,0,0,124,198,254,192,192,198,124,0,0,0,0,0,0,56,108,100,96,240,96,96,96,96,240,0,0,0,0,0,0,0,0,0,118,204,204,204,204,204,124,12,204,120,0,0,0,224,96,96,108,118,102,102,102,102,230,0,0,0,0,0,0,24,24,0,56,24,24,24,24,24,60,0,0,0,0,0,0,6,6,0,14,6,6,6,6,6,6,102,102,60,0,0,0,224,96,96,102,108,120,120,108,102,230,0,0,0,0,0,0,56,24,24,24,24,24,24,24,24,60,0,0,0,0,0,0,0,0,0,236,254,214,214,214,214,198,0,0,0,0,0,0,0,0,0,220,102,102,102,102,102,102,0,0,0,0,0,0,0,0,0,124,198,198,198,198,198,124,0,0,0,0,0,0,0,0,0,220,102,102,102,102,102,124,96,96,240,0,0,0,0,0,0,118,204,204,204,204,204,124,12,12,30,0,0,0,0,0,0,220,118,102,96,96,96,240,0,0,0,0,0,0,0,0,0,124,198,96,56,12,198,124,0,0,0,0,0,0,16,48,48,252,48,48,48,48,54,28,0,0,0,0,0,0,0,0,0,204,204,204,204,204,204,118,0,0,0,0,0,0,0,0,0,102,102,102,102,102,60,24,0,0,0,0,0,0,0,0,0,198,198,214,214,214,254,108,0,0,0,0,0,0,0,0,0,198,108,56,56,56,108,198,0,0,0,0,0,0,0,0,0,198,198,198,198,198,198,126,6,12,248,0,0,0,0,0,0,254,204,24,48,96,198,254,0,0,0,0,0,0,14,24,24,24,112,24,24,24,24,14,0,0,0,0,0,0,24,24,24,24,24,24,24,24,24,24,24,24,0,0,0,0,112,24,24,24,14,24,24,24,24,112,0,0,0,0,0,0,118,220,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,16,56,108,198,198,198,254,0,0,0,0,0 };
uint32_t fb_x, fb_y;
#endif

/**
 * Initialize the console
 */
void console_init(void)
{
    uint32_t r;

    /* initialize UART */
    *UART0_CR = 0;          /* turn off UART0 */
#ifdef CONSOLE_SERIAL
    /* set up clock for consistent divisor values */
    mbox[0] = 8*4;
    mbox[1] = MBOX_REQUEST;
    mbox[2] = 0x38002;      /* set clock rate */
    mbox[3] = 12;
    mbox[4] = 8;
    mbox[5] = 2;            /* UART clock */
    mbox[6] = 4000000;      /* 4Mhz */
    mbox[7] = 0;            /* set turbo */
    mbox_call(MBOX_CH_PROP);
    r = *GPFSEL1;
    r &= ~((7<<12)|(7<<15));/* gpio14, gpio15 */
    r |= (4<<12)|(4<<15);   /* alt0 */
    *GPFSEL1 = r;
    *GPPUD = 0;             /* enable pins 14 and 15 */
    for(r = 0; r < 150; r++) __asm__ __volatile__("nop");
    *GPPUDCLK0 = (1<<14)|(1<<15);
    for(r = 0; r < 150; r++) __asm__ __volatile__("nop");
    *GPPUDCLK0 = 0;         /* flush GPIO setup */
    *UART0_ICR = 0x7FF;     /* clear interrupts */
    *UART0_IBRD = 2;        /* 115200 baud */
    *UART0_FBRD = 0xB;
    *UART0_LCRH = 0x03<<5;  /* 8n1 */
    *UART0_CR = 0x301;      /* enable Tx, Rx, FIFO */
#endif
#ifdef CONSOLE_FB
    fb_x = fb_y = 4;
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
#ifdef CONSOLE_SERIAL
    do{ __asm__ __volatile__("nop");} while(*UART0_FR&0x20); *UART0_DR=c;
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

/**************** EMMC SDCard functions ****************/

#define EMMC_ARG2           ((volatile uint32_t*)(emmc_base+0x00000000))
#define EMMC_BLKSIZECNT     ((volatile uint32_t*)(emmc_base+0x00000004))
#define EMMC_ARG1           ((volatile uint32_t*)(emmc_base+0x00000008))
#define EMMC_CMDTM          ((volatile uint32_t*)(emmc_base+0x0000000C))
#define EMMC_RESP0          ((volatile uint32_t*)(emmc_base+0x00000010))
#define EMMC_RESP1          ((volatile uint32_t*)(emmc_base+0x00000014))
#define EMMC_RESP2          ((volatile uint32_t*)(emmc_base+0x00000018))
#define EMMC_RESP3          ((volatile uint32_t*)(emmc_base+0x0000001C))
#define EMMC_DATA           ((volatile uint32_t*)(emmc_base+0x00000020))
#define EMMC_STATUS         ((volatile uint32_t*)(emmc_base+0x00000024))
#define EMMC_CONTROL0       ((volatile uint32_t*)(emmc_base+0x00000028))
#define EMMC_CONTROL1       ((volatile uint32_t*)(emmc_base+0x0000002C))
#define EMMC_INTERRUPT      ((volatile uint32_t*)(emmc_base+0x00000030))
#define EMMC_INT_MASK       ((volatile uint32_t*)(emmc_base+0x00000034))
#define EMMC_INT_EN         ((volatile uint32_t*)(emmc_base+0x00000038))
#define EMMC_CONTROL2       ((volatile uint32_t*)(emmc_base+0x0000003C))
#define EMMC_SLOTISR_VER    ((volatile uint32_t*)(emmc_base+0x000000FC))
#define CMD_NEED_APP        0x80000000
#define CMD_RSPNS_48        0x00020000
#define CMD_ERRORS_MASK     0xfff9c004
#define CMD_RCA_MASK        0xffff0000
#define CMD_GO_IDLE         0x00000000
#define CMD_ALL_SEND_CID    0x02010000
#define CMD_SEND_REL_ADDR   0x03020000
#define CMD_CARD_SELECT     0x07030000
#define CMD_SEND_IF_COND    0x08020000
#define CMD_STOP_TRANS      0x0C030000
#define CMD_READ_SINGLE     0x11220010
#define CMD_READ_MULTI      0x12220032
#define CMD_SET_BLOCKCNT    0x17020000
#define CMD_APP_CMD         0x37000000
#define CMD_SET_BUS_WIDTH   (0x06020000|CMD_NEED_APP)
#define CMD_SEND_OP_COND    (0x29020000|CMD_NEED_APP)
#define CMD_SEND_SCR        (0x33220010|CMD_NEED_APP)
#define SR_READ_AVAILABLE   0x00000800
#define SR_DAT_INHIBIT      0x00000002
#define SR_CMD_INHIBIT      0x00000001
#define SR_APP_CMD          0x00000020
#define INT_DATA_TIMEOUT    0x00100000
#define INT_CMD_TIMEOUT     0x00010000
#define INT_READ_RDY        0x00000020
#define INT_CMD_DONE        0x00000001
#define INT_ERROR_MASK      0x017E8000
#define C0_SPI_MODE_EN      0x00100000
#define C0_HCTL_HS_EN       0x00000004
#define C0_HCTL_DWITDH      0x00000002
#define C1_SRST             0x07000000
#define C1_SRST_DATA        0x04000000
#define C1_SRST_CMD         0x02000000
#define C1_SRST_HC          0x01000000
#define C1_TOUNIT_DIS       0x000f0000
#define C1_TOUNIT_MAX       0x000e0000
#define C1_CLK_GENSEL       0x00000020
#define C1_CLK_EN           0x00000004
#define C1_CLK_STABLE       0x00000002
#define C1_CLK_INTLEN       0x00000001
#define HOST_SPEC_NUM       0x00ff0000
#define HOST_SPEC_NUM_SHIFT 16
#define HOST_SPEC_V3        2
#define HOST_SPEC_V2        1
#define HOST_SPEC_V1        0
#define SCR_SD_BUS_WIDTH_4  0x00000400
#define SCR_SUPP_SET_BLKCNT 0x02000000
#define SCR_SUPP_CCS        0x00000001
#define ACMD41_VOLTAGE      0x00ff8000
#define ACMD41_CMD_COMPLETE 0x80000000
#define ACMD41_CMD_CCS      0x40000000
#define ACMD41_ARG_HC       0x51ff8000
#define SD_OK                0
#define SD_TIMEOUT          -1
#define SD_ERROR            -2

uint32_t sd_scr[2], sd_ocr, sd_rca, sd_hv;
int sd_err;

uint64_t cntfrq;
/* delay cnt microsec */
void delayms(uint32_t cnt) {
    uint64_t t,r;
    if(!cntfrq) __asm__ __volatile__ ("mrs %0, cntfrq_el0" : "=r" (cntfrq));
    __asm__ __volatile__ ("mrs %0, cntpct_el0" : "=r" (t));
    t+=((cntfrq/1000)*cnt)/1000;do{__asm__ __volatile__ ("mrs %0, cntpct_el0" : "=r" (r));}while(r<t);
}

/**
 * Wait for data or command ready
 */
int sd_status(uint32_t mask)
{
    int cnt = 500000; while((*EMMC_STATUS & mask) && !(*EMMC_INTERRUPT & INT_ERROR_MASK) && cnt--) delayms(1);
    return (cnt <= 0 || (*EMMC_INTERRUPT & INT_ERROR_MASK)) ? SD_ERROR : SD_OK;
}

/**
 * Wait for interrupt
 */
int sd_int(uint32_t mask)
{
    uint32_t r, m=mask | INT_ERROR_MASK;
    int cnt = 1000000; while(!(*EMMC_INTERRUPT & m) && cnt--) delayms(1);
    r=*EMMC_INTERRUPT;
    if(cnt<=0 || (r & INT_CMD_TIMEOUT) || (r & INT_DATA_TIMEOUT) ) { *EMMC_INTERRUPT=r; return SD_TIMEOUT; } else
    if(r & INT_ERROR_MASK) { *EMMC_INTERRUPT=r; return SD_ERROR; }
    *EMMC_INTERRUPT=mask;
    return 0;
}

/**
 * Send a command
 */
int sd_cmd(uint32_t code, uint32_t arg)
{
    uint32_t r=0;
    sd_err=SD_OK;
    if(code&CMD_NEED_APP) {
        r=sd_cmd(CMD_APP_CMD|(sd_rca?CMD_RSPNS_48:0),sd_rca);
        if(sd_rca && !r) { printf("EMMC: failed to send SD APP command\r\n"); sd_err=SD_ERROR;return 0;}
        code &= ~CMD_NEED_APP;
    }
    if(sd_status(SR_CMD_INHIBIT)) { printf("EMMC: busy, timed out\r\n"); sd_err= SD_TIMEOUT;return 0;}
    *EMMC_INTERRUPT=*EMMC_INTERRUPT; *EMMC_ARG1=arg; *EMMC_CMDTM=code;
    if(code==CMD_SEND_OP_COND) delayms(1000); else
    if(code==CMD_SEND_IF_COND || code==CMD_APP_CMD) delayms(100);
    if((r=sd_int(INT_CMD_DONE))) {printf("EMMC: failed to send command\r\n");sd_err=r;return 0;}
    r=*EMMC_RESP0;
    if(code==CMD_GO_IDLE || code==CMD_APP_CMD) return 0; else
    if(code==(CMD_APP_CMD|CMD_RSPNS_48)) return r&SR_APP_CMD; else
    if(code==CMD_SEND_OP_COND) return r; else
    if(code==CMD_SEND_IF_COND) return r==arg? SD_OK : SD_ERROR; else
    if(code==CMD_ALL_SEND_CID) {r|=*EMMC_RESP3; r|=*EMMC_RESP2; r|=*EMMC_RESP1; return r; } else
    if(code==CMD_SEND_REL_ADDR) {
        sd_err=(((r&0x1fff))|((r&0x2000)<<6)|((r&0x4000)<<8)|((r&0x8000)<<8))&CMD_ERRORS_MASK;
        return r&CMD_RCA_MASK;
    }
    return r&CMD_ERRORS_MASK;
}

/**
 * Load a sector from boot drive using EMMC
 */
int sd_loadsec(uint64_t lba, void *dst)
{
    int r,d;
    uint32_t *buf=(uint32_t *)dst;

#ifdef SD_DEBUG
    printf("EMMC: sd_loadsec lba %d dst %lx\r\n", lba, dst);
#endif
    if(sd_status(SR_DAT_INHIBIT)) {sd_err=SD_TIMEOUT; return 0;}
    *EMMC_BLKSIZECNT = (1 << 16) | 512;
    sd_cmd(CMD_READ_SINGLE, sd_scr[0] & SCR_SUPP_CCS ? lba : lba << 9);
    if(sd_err) return 0;
    if((r=sd_int(INT_READ_RDY))){printf("\rEMMC: timeout waiting for ready to read\n");sd_err=r;return 0;}
    for(d=0;d<128;d++) buf[d] = *EMMC_DATA;
    return sd_err!=SD_OK;
}

/**
 * set SD clock to frequency in Hz
 */
int sd_clk(uint32_t f)
{
    uint32_t d,c=41666666/f,x,s=32,h=0;
    int cnt = 100000;
    while((*EMMC_STATUS & (SR_CMD_INHIBIT|SR_DAT_INHIBIT)) && cnt--) delayms(1);
    if(cnt<=0) {printf("EMMC: timeout waiting for inhibit flag\r\n"); return SD_ERROR; }

    *EMMC_CONTROL1 &= ~C1_CLK_EN; delayms(10);
    x=c-1; if(!x) s=0; else {
        if(!(x & 0xffff0000u)) { x <<= 16; s -= 16; }
        if(!(x & 0xff000000u)) { x <<= 8;  s -= 8; }
        if(!(x & 0xf0000000u)) { x <<= 4;  s -= 4; }
        if(!(x & 0xc0000000u)) { x <<= 2;  s -= 2; }
        if(!(x & 0x80000000u)) { x <<= 1;  s -= 1; }
        if(s>0) s--;
        if(s>7) s=7;
    }
    if(sd_hv>HOST_SPEC_V2) d=c; else d=(1<<s);
    if(d<=2) {d=2;s=0;}
#ifdef SD_DEBUG
    printf("sd_clk divisor %x, shift %x\r\n", d, s);
#endif
    if(sd_hv>HOST_SPEC_V2) h=(d&0x300)>>2;
    d=(((d&0x0ff)<<8)|h);
    *EMMC_CONTROL1=(*EMMC_CONTROL1&0xffff003f)|d; delayms(10);
    *EMMC_CONTROL1 |= C1_CLK_EN; delayms(10);
    cnt=10000; while(!(*EMMC_CONTROL1 & C1_CLK_STABLE) && cnt--) delayms(10);
    if(cnt<=0) {printf("EMMC: failed to get stable clock\r\n");return SD_ERROR;}
    return SD_OK;
}

/**
 * initialize EMMC to read SDHC card
 */
int sd_init()
{
    long r,cnt,ccs=0;
    sd_hv = (*EMMC_SLOTISR_VER & HOST_SPEC_NUM) >> HOST_SPEC_NUM_SHIFT;
#ifdef SD_DEBUG
    printf("EMMC: GPIO set up hcl ver %d\r\n", sd_hv);
#endif
    if(sd_hv<HOST_SPEC_V2) {printf("EMMC: SDHCI version too old\r\n"); return SD_ERROR;}
    /* Reset the card. */
    *EMMC_CONTROL0 = 0;
    r = *EMMC_CONTROL1; r |= C1_SRST_HC; r &= ~(C1_CLK_EN | C1_CLK_INTLEN); *EMMC_CONTROL1 = r;
    cnt=10000; do{delayms(10);} while( (*EMMC_CONTROL1 & C1_SRST) && cnt-- );
    if(cnt<=0) {printf("EMMC: failed to reset EMMC\r\n"); return SD_ERROR;}
#ifdef SD_DEBUG
    printf("EMMC: reset OK\n");
#endif
    *EMMC_CONTROL0 = 0xF << 8; /* set voltage to 3.3 */
    *EMMC_CONTROL1 |= C1_CLK_INTLEN | C1_TOUNIT_MAX;
    delayms(10);
    /* Set clock to setup frequency. */
    if((r=sd_clk(400000))) return r;
    *EMMC_INT_EN   = 0xffffffff;
    *EMMC_INT_MASK = 0xffffffff;
    sd_scr[0]=sd_scr[1]=sd_rca=sd_err=0;
    sd_cmd(CMD_GO_IDLE,0);
    if(sd_err) return sd_err;

    sd_cmd(CMD_SEND_IF_COND,0x000001AA);
    if(sd_err) return sd_err;
    cnt=6; r=0; while(!(r&ACMD41_CMD_COMPLETE) && cnt--) {
        delayms(1);
        r=sd_cmd(CMD_SEND_OP_COND,ACMD41_ARG_HC);
#ifdef SD_DEBUG
        printf("EMMC: CMD_SEND_OP_COND returned ");
        if(r&ACMD41_CMD_COMPLETE) printf("COMPLETE ");
        if(r&ACMD41_VOLTAGE) printf("VOLTAGE ");
        if(r&ACMD41_CMD_CCS) printf("CCS ");
        printf("%x\r\n", r);
#endif
        if(sd_err!=SD_TIMEOUT && sd_err!=SD_OK ) {printf("EMMC: EMMC ACMD41 returned error\r\n"); return sd_err;}
    }
    if(!(r&ACMD41_CMD_COMPLETE) || !cnt ) return SD_TIMEOUT;
    if(!(r&ACMD41_VOLTAGE)) return SD_ERROR;
    if(r&ACMD41_CMD_CCS) ccs=SCR_SUPP_CCS;

    sd_cmd(CMD_ALL_SEND_CID,0);

    sd_rca = sd_cmd(CMD_SEND_REL_ADDR,0);
#ifdef SD_DEBUG
    printf("EMMC: CMD_SEND_REL_ADDR returned %x\r\n", sd_rca);
#endif
    if(sd_err) return sd_err;

    if((r=sd_clk(25000000))) return r;

    sd_cmd(CMD_CARD_SELECT,sd_rca);
    if(sd_err) return sd_err;

    if(sd_status(SR_DAT_INHIBIT)) return SD_TIMEOUT;
    *EMMC_BLKSIZECNT = (1<<16) | 8;
    sd_cmd(CMD_SEND_SCR,0);
    if(sd_err) return sd_err;
    if(sd_int(INT_READ_RDY)) return SD_TIMEOUT;

    r=0; cnt=100000; while(r<2 && cnt) {
        if( *EMMC_STATUS & SR_READ_AVAILABLE )
            sd_scr[r++] = *EMMC_DATA;
        else
            delayms(1);
    }
    if(r!=2) return SD_TIMEOUT;
    if(sd_scr[0] & SCR_SD_BUS_WIDTH_4) {
        sd_cmd(CMD_SET_BUS_WIDTH,sd_rca|2);
        if(sd_err) return sd_err;
        *EMMC_CONTROL0 |= C0_HCTL_DWITDH;
    }
    /* add software flag */
#ifdef SD_DEBUG
    printf("EMMC: supports ");
    if(sd_scr[0] & SCR_SUPP_SET_BLKCNT) printf("SET_BLKCNT ");
    if(ccs) printf("CCS ");
    printf("\r\n");
#endif
    sd_scr[0]&=~SCR_SUPP_CCS;
    sd_scr[0]|=ccs;
    return SD_OK;
}

/**************** Common functions ****************/

/**
 * Load a sector from boot drive using EMMC
 */
int fw_loadsec(uint64_t lba, void *dst)
{
    return sd_loadsec(lba, dst);
}

/**
 * Allocate and zero out a page
 */
uint64_t fw_alloc(void)
{
    uint64_t page = file_buf;
    file_buf += 4096;
    memset((void*)page, 0, 4096);
    return page;
}

/**
 * Map virtual memory
 */
int fw_map(uint64_t phys, uint64_t virt, uint32_t size)
{
    uint64_t end = virt + size, *ptr, *next = NULL, orig = file_buf, type;

    /* is this a canonical address? We handle virtual memory up to 256TB */
    if(!pt || ((virt >> 48L) != 0x0000 && (virt >> 48L) != 0xffff)) return 0;
    /* if we're mapping the framebuffer, use device SH=2 memory type */
    type = 0x03 | (1<<10) | (file_buf < 0x20000 ? (2<<8) | (1<<2) | (1L<<54) : (3<<8));

    /* walk the page tables and add the missing pieces */
    for(virt &= ~4095, phys &= ~4095; virt < end; virt += 4096) {
        /* 1G */
        ptr = &pt[((virt >> 48L) ? 512 : 0) + ((virt >> 30L) & 511)];
        if(!*ptr) { if(!(*ptr = fw_alloc())) return 0; else *ptr |= 3|(3<<8)|(1<<10); }
        /* 2M if we previously had a large page here, split it into 4K pages */
        ptr = (uint64_t*)(*ptr & ~4095); ptr = &ptr[(virt >> 21L) & 511];
        if(!(*ptr & 0x2)) { if(!(*ptr = fw_alloc())) return 0; else *ptr |= 3|(3<<8)|(1<<10); }
        /* 4K */
        ptr = (uint64_t*)(*ptr & ~4095); ptr = &ptr[(virt >> 12L) & 511];
        /* if this page is already mapped, that means the kernel has invalid, overlapping segments */
        if(!*ptr) { *ptr = (uint64_t)next; next = ptr; }
    }
    /* resolve the linked list */
    for(end = ((phys == orig ? file_buf : phys) + size - 1) & ~4095; next; end -= 4096, next = ptr) {
        ptr = (uint64_t*)*next; *next = end | type;
    }
    return 1;
}

/**
 * Initialize firmware related stuff
 */
void fw_init(void)
{
    uint64_t i=0, ms, me;

    /* get the base address */
    __asm__ __volatile__("mrs %0, midr_el1;":"=r"(mmio_base)::);
    switch(mmio_base & 0xFFF0) {
        case 0xD030: rpi = 3; mmio_base = 0x3F000000; emmc_base = 0x3F300000; break;     /* Raspberry Pi 3 */
        default:     rpi = 4; mmio_base = 0xFE000000; emmc_base = 0xFE340000; break;     /* Raspberry Pi 4 */
    }
    file_base = (uint64_t)_bss_end;
    /* if we got a DTB from the firmware, move it out of the way */
    if(dtb_base && (uintptr_t)dtb_base < 2*1024*1024 && dtb_base[0] == 0xD0 && dtb_base[1] == 0x0D &&
      dtb_base[2] == 0xFE && dtb_base[3] == 0xED) {
        i = dtb_base[7] | (dtb_base[6] << 8) | (dtb_base[5] << 16);
        memcpy((void*)file_base, dtb_base, i);
        dtb_base = (uint8_t*)(uintptr_t)file_base; file_base += 4096 + ((i + 4095) & ~4095);
    } else dtb_base = 0;
    /* initialize screen and console */
    mbox_lfb(0, 0, 0);
    fb_w = vidmode.framebuffer_width; fb_h = vidmode.framebuffer_height; fb_bpp = vidmode.framebuffer_bpp;
    console_init();
    /* set up paging */
    ms = mmio_base >> 21; me = (mmio_base + 0x800000) >> 21;
    pt = (uint64_t*)0x1000;
    memset(pt, 0, 8 * 4096);
    /* TTBR0 */
    for(i = 0; i < 4; i++)
        pt[i] = (uintptr_t)pt + ((i + 2) * 4096) + (3|(3<<8)|(1<<10));
    for(i = 0; i < 4 * 512; i++) pt[1024 + i] = (uintptr_t)i * 2 * 1024 * 1024 +
        /* if we're mapping the mmio area, use device SH=2 memory type */
        (1 | (1<<10) | (i >= ms && i < me ? (2<<8) | (1<<2) | (1L<<54) : (3<<8)));
    /* dynamically map framebuffer */
    if(vidmode.framebuffer_addr) {
        /* map new pages from the page table area */
        file_buf = 0x7000;
        fw_map(vidmode.framebuffer_addr, vidmode.framebuffer_addr,
            (vidmode.framebuffer_pitch * vidmode.framebuffer_height + 4095) & ~4095);
    }
    file_buf = file_base;
    /* TTBR1 */
    /* pt[512] = nothing for now; */
}

/**
 * Initialize root file system
 */
void fw_fsinit(void)
{
    uint32_t i, j, n;
    uint64_t k, l;

    /* initialize SDCard */
    sd_init();
    /* get boot partition's root directory */
    fw_loadsec(1, &vbr);
    root_dir = 0;
    if(!memcmp(&vbr, EFI_PTAB_HEADER_ID, 8)) {
        /* found GPT */
        j = ((gpt_header_t*)&vbr)->SizeOfPartitionEntry;
        n = ((gpt_header_t*)&vbr)->NumberOfPartitionEntries;
        l = ((gpt_header_t*)&vbr)->PartitionEntryLBA;
        /* look for ESP */
        for(k = 0; n && !root_dir; k++) {
            fw_loadsec(l + k, &vbr);
            for(i = 0; n && i + j <= 512; i += j, n--) {
                /* does ESP type match? */
                if(!memcmp(&((gpt_entry_t*)&vbr[i])->PartitionTypeGUID, &espGuid, sizeof(guid_t))) {
                    root_dir = ((gpt_entry_t*)&vbr[i])->StartingLBA;
                    memcpy(&bootuuid, &(((gpt_entry_t*)&vbr[i])->UniquePartitionGUID), sizeof(guid_t));
                    break;
                }
            }
        }
    } else {
        /* fallback to MBR partitioning scheme */
        fw_loadsec(0, &vbr);
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
        fw_loadsec(root_dir, &vbr);
        bpb = (esp_bpb_t*)&vbr;
        if(vbr[510] != 0x55 || vbr[511] != 0xAA || vbr[11] || vbr[12] != 2 || !bpb->spc || bpb->spf16 || !bpb->spf32)
            root_dir = 0;
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
 * Get the next cluster from FAT
 */
uint32_t fw_nextclu(uint32_t clu)
{
    uint64_t i;

    if(clu < 2 || clu >= 0x0FFFFFF8) return 0;
    if(clu < fat_cache || clu > fat_cache + 1023) {
        fat_cache = clu & ~1023;
        for(i = 0; i < 8; i++) fw_loadsec(fat_lba + (fat_cache >> 7) + i, &fat[i << 7]);
    }
    clu = fat[clu - fat_cache];
    return clu < 2 || clu >= 0x0FFFFFF8 ? 0 : clu;
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
    char *S;

    if(!fn || !*fn) return 0;
    /* UTF-8 to WCHAR */
    for(S = fn, d = wcname; *S && *S != ' ' && *S != '\r' && *S != '\n' && d < &wcname[PATH_MAX - 2]; d++) {
        if((*S & 128) != 0) {
            if(!(*S & 32)) { c = ((*S & 0x1F)<<6)|(*(S+1) & 0x3F); S++; } else
            if(!(*S & 16)) { c = ((*S & 0xF)<<12)|((*(S+1) & 0x3F)<<6)|(*(S+2) & 0x3F); S += 2; } else
            if(!(*S & 8)) { c = ((*S & 0x7)<<18)|((*(S+1) & 0x3F)<<12)|((*(S+2) & 0x3F)<<6)|(*(S+3) & 0x3F); *S += 3; }
            else c = 0;
        } else c = *S;
        S++; if(c == '\\' && *S == ' ') { c = ' '; S++; }
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
                clu = fw_nextclu(clu);
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
 * Read data from file
 */
uint64_t fw_read(uint64_t offs, uint64_t size, void *buf)
{
    uint64_t lba = 0, rem, o;
    uint32_t clu = file_clu, nc, ns = 0, os = 0, rs = 512;
    uint8_t secleft = 0;

    if(file_clu < 2 || offs >= file_size || !size || !buf) return 0;
    if(offs + size > file_size) size = file_size - offs;
    rem = size;

    pb_init(size);
    if(offs) {
        nc = offs / (bpb->spc << 9); o = offs % (bpb->spc << 9);
        ns = o >> 9; os = o & 0x1ff; rs = 512 - os;
        if(nc) { while(nc-- && clu) { clu = fw_nextclu(clu); } if(!clu) return 0; }
        secleft = bpb->spc - ns - 1;
        lba = clu * bpb->spc + ns - 1 + data_lba;
    }
    while(rem && !rq) {
        /* check for user interruption */
        if(!bkp && !rq && !(*UART0_FR & 0x10)) { rq = 1; break; }
        if(secleft) { secleft--; lba++; }
        else {
            if(!clu) break;
            secleft = bpb->spc - 1;
            lba = clu * bpb->spc + data_lba;
            clu = fw_nextclu(clu);
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
 * Close file
 */
void fw_close(void)
{
    file_clu = file_size = 0;
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

    kernel = NULL;
    tags_buf = (uint8_t*)(uintptr_t)0x20000;
    /* as a fallback, we try to load the first menuentry from easyboot's configuration */
    if(fw_open("simpleboot.cfg") || (!bkp && fw_open("easyboot/menu.cfg"))) {
        conf_buf = (char*)tags_buf;
        tags_buf += (file_size + 7) & ~7;
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
                        logo_buf = tags_buf;
                        tags_buf += (file_size + 7) & ~7;
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
    if(!kernel) kernel = (char*)defkernel;
    if(!bkp && (volatile char)definitrd[63] == 1) smp = 1;
}

/**
 * Detect config file independent configuration and generate tags for them
 */
void fw_loadsetup()
{
    multiboot_tag_loader_t *stag;
    multiboot_tag_mmap_t *mtag;
    char *c;

    mod_buf = 0;
    file_buf = file_base;
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
        mbox[0]=8*4;
        mbox[1]=0;
        mbox[2]=0x10005; /* get memory size */
        mbox[3]=8;
        mbox[4]=0;
        mbox[5]=0;
        mbox[6]=0;
        mbox[7]=0;
        if(mbox_call(MBOX_CH_PROP)) ram = mbox[6];
        else ram = 64*1024*1024;
        ram &= ~(2 * 1024 * 1024 - 1);
        /* generate memory map tag */
        mtag = (multiboot_tag_mmap_t*)tags_ptr;
        mtag->type = MULTIBOOT_TAG_TYPE_MMAP;
        mtag->entry_size = sizeof(multiboot_mmap_entry_t);
        mtag->entry_version = 0;
        mtag->entries[0].base_addr = 0;
        mtag->entries[0].length = 0x400;
        mtag->entries[0].type = MULTIBOOT_MEMORY_RESERVED;
        mtag->entries[0].reserved = 0;
        mtag->entries[1].base_addr = 0x400;
        mtag->entries[1].length = ram - 0x400;
        mtag->entries[1].type = MULTIBOOT_MEMORY_AVAILABLE;
        mtag->entries[1].reserved = 0;
        mtag->entries[2].base_addr = mmio_base;
        mtag->entries[2].length = 0x00800000;
        mtag->entries[2].type = MULTIBOOT_MEMORY_RESERVED;
        mtag->entries[2].reserved = EfiMemoryMappedIO;
        mtag->size = sizeof(multiboot_tag_mmap_t) + 3 * sizeof(multiboot_mmap_entry_t);
        tags_ptr += (mtag->size + 7) & ~7;
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
                    /* if it's a gzip compressed module, then load it at position + uncompressed size,
                     * and uncompress to position. Compressed buffer will be overwritten by the next module. */
                    uncomp = 0;
                    if(tmp[0] == 0x1f && tmp[1] == 0x8b)
                        fw_read(file_size - 4, 4, (void*)&uncomp);
                    else
                    if(tmp[0] == 'G' && tmp[1] == 'U' && tmp[2] == 'D' && tmp[8] == 0x78)
                        uncomp = (((tmp[4] | (tmp[5] << 8)) + 7) & ~7) + ((tmp[6] | (tmp[7] << 8)) << 4);
                    ptr = (uint8_t*)file_buf;
                    if(uncomp) {
                        unc_buf = file_buf;
                        file_buf += (uncomp + 4095) & ~4095;
                        mod_buf = file_buf;
                    } else {
                        unc_buf = 0;
                        mod_buf = file_buf;
                        file_buf += (file_size + 4095) & ~4095;
                    }
                    if(verbose) printf("Loading module '%S' (%ld bytes)...\r\n", wcname, file_size);
                    fw_read(0, file_size, (void*)mod_buf);
                    if(unc_buf) {
                        if(verbose) printf("Uncompressing module '%S' (%d bytes)...\r\n", wcname, uncomp);
                        uncompress((uint8_t*)mod_buf, (uint8_t*)unc_buf, uncomp);
                    }
                    /* if it's a DTB, DSDT or a GUDT, don't add it to the modules list, add it to the ACPI tables */
                    if(ptr[0] == 0xD0 && ptr[1] == 0x0D && ptr[2] == 0xFE && ptr[3] == 0xED) {
                        if(verbose) printf("DTB detected...\r\n");
                        dtb_base = ptr;
                        /* make sure we have enough space after the dtb, because we'll have to patch it */
                        file_buf += 4096;
                    } else
                    if(((ptr[0] == 'D' && ptr[1] == 'S') || (ptr[0] == 'G' && ptr[1] == 'U')) && ptr[2] == 'D' && ptr[3] == 'T') {
                        if(verbose) printf("%c%cDT detected...\n", ptr[0], ptr[1]);
                        dtb_base = ptr;
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
                    fw_close();
                } else if(verbose) printf("WARNING: unable to load '%S'\r\n", wcname);
            }
            /* go to the next line */
            s = e;
        }
    }
    /* if no modules were loaded, but we have a default initrd name, try to add that */
    if(!n && !f) { f = 1; if((volatile char)definitrd[0]) { a = (char*)definitrd; for(e = a; *e; e++){} goto ldinitrd; } }
    if(!n && f == 1) { f = 2; a = bkp ? "rpi/initrd.bak" : "rpi/initrd"; e = a + (bkp ? 16 : 12); goto ldinitrd; }
}

/**
 * Load a kernel segment
 */
int fw_loadseg(uint32_t offs, uint32_t filesz, uint64_t vaddr, uint32_t memsz)
{
    uint64_t top;
    uint8_t *buf = (uint8_t*)(uintptr_t)vaddr;
    uint32_t size;

    if(!memsz || !file_size) return 1;
    if(verbose > 1) printf("  segment %08x[%08x] -> %08x[%08x]\r\n", offs, filesz, vaddr, memsz);
    size = (memsz + (vaddr & 4095) + 4095) & ~4095;
    /* no overwriting of the loader data */
    if(vaddr < _bss_end) goto err;
    if(vaddr > ram) {
        /* possibly a higher-half kernel's segment, we must map it */
        if(!fw_map(file_buf, vaddr, size)) goto err;
        buf = (void*)file_buf; file_buf += size;
    } else {
        /* make sure we load modules after the kernel to avoid any conflict */
        top = ((uintptr_t)buf + size + 4095) & ~4095; if(file_buf < top) file_buf = top;
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
    pe_hdr *pe;
    pe_sec *sec;
    uint8_t *ptr;
    uint64_t offs;
    int i;

    wcname[0] = 0;
    if(!((kernel && *kernel && fw_open(kernel)) || fw_open("rpi/core"))) {
        smp = 0;
        if(wcname[0]) printf("ERROR: kernel '%S' not found\r\n", wcname);
        else printf("ERROR: kernel not found\r\n");
        return 0;
    }
    fw_read(0, sizeof(kernel_buf), p);
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
        if(verbose) printf("Loading Multiboot2 ELF64 kernel '%S'...\r\n", wcname);
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
        kernel_mode = MODE_MB64;
        pe = (pe_hdr*)(p + ((mz_hdr*)p)->peaddr);
        offs = (uint32_t)pe->data.pe64.img_base;
        kernel_entry = offs + (uint8_t*)(uintptr_t)pe->entry_point;
        if(verbose) printf("Loading Multiboot2 PE64 kernel '%S'...\r\n", wcname);
        sec = (pe_sec*)((uint8_t*)pe + pe->opt_hdr_size + 24);
        for(i = 0; !rq && i < pe->sections && (uint8_t*)&sec[1] < kernel_buf + sizeof(kernel_buf); i++, sec++)
            /* the PE section vaddr field is only 32 bits, we must make sure that it properly sign extended to 64 bit */
            if(!fw_loadseg(sec->raddr, sec->rsiz, offs + (int64_t)(int32_t)sec->vaddr, sec->vsiz)) goto err;
    } else {
        printf("ERROR: unknown kernel format '%S'\r\n", wcname);
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
    rsdp_t *rsdp;
    register uint64_t reg;
    uint32_t ow, oh, ob, totalsize, off_dt, off_str, siz_str, siz_dt, tag, sz, ol = 0, nl = 0, cl = 0;
    uint8_t s, *p, *end, *l = NULL, *cmd = NULL;
    int i, n = 4;

    /* patch the command line in the dtb, Linux needs this */
    if(dtb_base && (dtb_base[0] == 0xD0 && dtb_base[1] == 0x0D && dtb_base[2] == 0xFE && dtb_base[3] == 0xED)) {
        /* proof that Linux developers are incompetent and total morons: where did they put the most likely to change string?
         * in the string table maybe? Nah, that would be outrageous! They barfed that right in the middle of a binary blob!!! */
        totalsize = dtb_base[7] | (dtb_base[6] << 8) | (dtb_base[5] << 16);
        off_dt = dtb_base[11] | (dtb_base[10] << 8) | (dtb_base[9] << 16);
        off_str = dtb_base[15] | (dtb_base[14] << 8) | (dtb_base[13] << 16);
        siz_str = dtb_base[35] | (dtb_base[34] << 8) | (dtb_base[33] << 16);
        siz_dt = dtb_base[39] | (dtb_base[38] << 8) | (dtb_base[37] << 16);
        p = dtb_base + off_dt; end = dtb_base + off_dt + siz_dt;
        /* failsafe, we assume that the string table is the last section in the blob */
        if(off_str >= off_dt + siz_dt) {
            /* get size of the new command line */
            if(cmdline && *cmdline) for(; cmdline[cl]; cl++);
            cl++; nl = (cl + 3) & ~3;
            /* locate the command line node property */
            while(p < end) {
                tag = p[3] | (p[2] << 8) | (p[1] << 16) | (p[0] << 24); p += 4;
                if(tag == 9) break;
                if(tag == 1) {
                    for(l = p; p < end && *p; p++);
                    p = (uint8_t*)(((uintptr_t)p + 3) & ~3);
                } else
                if(tag == 2) l = NULL; else
                if(tag == 3) {
                    sz = p[3] | (p[2] << 8) | (p[1] << 16); p += 4; tag = p[3] | (p[2] << 8) | (p[1] << 16); p += 4;
                    if(tag < siz_str - 1 && l && !memcmp(l, "chosen", 7) && !memcmp(dtb_base + off_str + tag, "bootargs", 9)) {
                        cmd = p; ol = (sz + 3) & ~3; break;
                    }
                    p = (uint8_t*)(((uintptr_t)p + sz + 3) & ~3);
                }
            }
            /* if we haven't found it */
            if(!cmd) {
                /* we need to add the property key to the string table, and a chosen node at the end of the node tree */
                tag = siz_str; siz_str += 9; totalsize += 9;
                memcpy(dtb_base + tag, "bootargs", 9);
                cmd = dtb_base + off_dt + siz_dt;
                for(p = dtb_base + totalsize, l = dtb_base + totalsize + nl + 28; p >= cmd; p++, l++) *l = *p;
                memcpy(cmd, "\0\0\0\1chosen\0\0\0\0\0\3", 16); cmd += 16;
                cmd[3] = cl & 0xff; cmd[2] = (cl >> 8) & 0xff; cmd[1] = (cl >> 8) & 0xff; cmd[0] = 0; cmd += 4;
                cmd[3] = tag & 0xff; cmd[2] = (tag >> 8) & 0xff; cmd[1] = (tag >> 8) & 0xff; cmd[0] = 0; cmd += 4;
                if(cmdline) memcpy(cmd, cmdline, cl - 1);
                memset(cmd + cl - 1, 0, nl - cl + 1);
                memcpy(cmd + nl, "\0\0\0\2", 4);
                siz_dt += nl + 28; totalsize += nl + 28; off_str += nl + 28;
            } else {
                /* update the command line somewhere in the middle... */
                if(nl < ol) memcpy(cmd + nl, cmd + ol, totalsize - ol); else
                if(nl > ol) for(p = dtb_base + totalsize, l = dtb_base + totalsize + nl - ol; p >= cmd + ol; p++, l++) *l = *p;
                cmd[-5] = cl & 0xff; cmd[-6] = (cl >> 8) & 0xff; cmd[-7] = (cl >> 8) & 0xff; cmd[-8] = 0;
                if(cmdline) memcpy(cmd, cmdline, cl - 1);
                memset(cmd + cl - 1, 0, nl - cl + 1);
                siz_dt += nl - ol; totalsize += nl - ol; off_str += nl - ol;
            }
            /* write header back with new offset and size values */
            dtb_base[7] = totalsize & 0xff; dtb_base[6] = (totalsize >> 8) & 0xff; dtb_base[5] = (totalsize >> 16) & 0xff;
            dtb_base[15] = off_str & 0xff; dtb_base[14] = (off_str >> 8) & 0xff; dtb_base[13] = (off_str >> 16) & 0xff;
            dtb_base[35] = siz_str & 0xff; dtb_base[34] = (siz_str >> 8) & 0xff; dtb_base[33] = (siz_str >> 16) & 0xff;
            dtb_base[39] = siz_dt & 0xff; dtb_base[38] = (siz_dt >> 8) & 0xff; dtb_base[37] = (siz_dt >> 16) & 0xff;
        }
    }

    if(vidmode.framebuffer_addr) {
        if(vidmode.framebuffer_width != fb_w || vidmode.framebuffer_height != fb_h || vidmode.framebuffer_bpp != fb_bpp) {
            ow = vidmode.framebuffer_width; oh = vidmode.framebuffer_height; ob = vidmode.framebuffer_bpp;
            mbox_lfb(fb_w, fb_h, fb_bpp);
            if(!vidmode.framebuffer_addr) mbox_lfb(ow, oh, ob);
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
    }
    if(tags_ptr) {
        /* Get EDID info */
        mbox[0]=8*4;
        mbox[1]=0;
        mbox[2]=0x30020; /* get edid */
        mbox[3]=4;
        mbox[4]=0;
        mbox[5]=0;
        mbox[6]=0;
        mbox[7]=0;
        if(mbox_call(MBOX_CH_PROP) && mbox[3] > 128) {
            ((multiboot_tag_edid_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_EDID;
            ((multiboot_tag_edid_t*)tags_ptr)->size = mbox[3];
            memcpy(tags_ptr + 8, (void*)&mbox[6], mbox[3] - 8);
            tags_ptr += (mbox[3] + 7) & ~7;
        }
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
        rsdt.hdr.rev = 1; rsdt.hdr.size = sizeof(sdt_hdr_t) + sizeof(uint32_t); rsdt.table_ptr[0] = (uint32_t)(uintptr_t)&apic;
        if(dtb_base && (
          (dtb_base[0] == 0xD0 && dtb_base[1] == 0x0D && dtb_base[2] == 0xFE && dtb_base[3] == 0xED) ||
          (((dtb_base[0] == 'D' && dtb_base[1] == 'S') || (dtb_base[0] == 'G' && dtb_base[1] == 'U')) &&
          dtb_base[2] == 'D' && dtb_base[3] == 'T'))) {
            /* add fake FADT and DSDT tables to the ACPI list with the DTB data */
            rsdt.hdr.size += sizeof(uint32_t);
            rsdt.table_ptr[1] = (uint32_t)(uintptr_t)&fadt; /* DSDT is pointed by FADT, not RSDT */
            memset(&fadt, 0, sizeof(fadt_t));
            memcpy(&fadt.hdr.magic, "FACP", 4);
            fadt.hdr.rev = 1; fadt.hdr.size = sizeof(fadt_t); fadt.dsdt = (uint32_t)(uintptr_t)dtb_base;
            for(s = 0, i = 0; i < (int)sizeof(fadt); i++) { s += *(((uint8_t*)&fadt) + i); } fadt.hdr.chksum = 0x100 - s;
        }
        for(s = 0, i = 0; i < (int)sizeof(rsdt_t); i++) { s += *(((uint8_t*)&rsdt) + i); } rsdt.hdr.chksum = 0x100 - s;
        memset(&apic, 0, sizeof(apic_t));
        memcpy(&apic.hdr.magic, "APIC", 4);
        apic.hdr.rev = 1; apic.hdr.size = sizeof(sdt_hdr_t) + (n + 1) * 8;
        for(i = 0; i < n; i++) {
            apic.cpus[i].type = 0; apic.cpus[i].size = sizeof(cpu_entry_t);
            apic.cpus[i].acpi_id = i; apic.cpus[i].apic_id =  0xd8 + i * 8; apic.cpus[i].flags = i ? 2 : 1;
        }
        for(s = 0, i = 0; i < (int)apic.hdr.size; i++) { s += *(((uint8_t*)&apic) + i); } apic.hdr.chksum = 0x100 - s;
        tags_ptr += (((multiboot_tag_old_acpi_t*)tags_ptr)->size + 7) & ~7;
        /* multicore */
        if(smp) {
            ((multiboot_tag_smp_t*)tags_ptr)->type = MULTIBOOT_TAG_TYPE_SMP;
            ((multiboot_tag_smp_t*)tags_ptr)->size = sizeof(multiboot_tag_smp_t);
            ((multiboot_tag_smp_t*)tags_ptr)->numcores = n;
            ((multiboot_tag_smp_t*)tags_ptr)->running = n;
            ((multiboot_tag_smp_t*)tags_ptr)->bspid = 0;
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
    *((uint64_t*)0x558) = 0;
}

/**
 * Dummy exception handler
 */
void fw_exc(uint8_t excno, uint64_t esr, uint64_t elr, uint64_t spsr, uint64_t far, uint64_t sctlr, uint64_t tcr)
{
    register uint64_t r0, r1, r2, r3;
#ifdef CONSOLE_FB
    uint32_t i, j, x, y, b;
#endif
    if(!in_exc) {
        in_exc++;
        /* only report exceptions for the BSP */
        __asm__ __volatile__ ("mrs x8, mpidr_el1; and x8, x8, #3; cbz x8, 2f; 1: wfe; b 1b; 2:;" :::"x8");
        __asm__ __volatile__ ("msr ttbr0_el1, %0;tlbi vmalle1" ::"r"((uint64_t)&pt[0] + 1));
        __asm__ __volatile__ ("dsb ish; isb; mrs %0, sctlr_el1" :"=r"(r0)::);
        /* set mandatory reserved bits to disable cache */
        r0 &= ~((1 << 12) /* clear I */ | (1 << 2) /* clear C */);
        __asm__ __volatile__ ("msr sctlr_el1, %0; isb" ::"r"(r0));

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
        printf("Simpleboot Exception Handler\r\nException #%02x\r\n\r\n", excno);
#ifdef CONSOLE_FB
        fb_bg = 0;
#endif
        __asm__ __volatile__("mrs %0, sp_el0;mov %1, sp;mrs %2, ttbr0_el1;mrs %3, ttbr1_el1" :"=r"(r0),"=r"(r1),"=r"(r2),"=r"(r3)::);
        printf("  ESR %016x   ELR %016x\n SPSR %016x   FAR %016x\nSCTLR %016x   TCR %016x\n  SP0 %016x   SP1 %016x\n"
            "TTBR0 %016x TTBR1 %016x\n\n", esr, elr, spsr, far, sctlr, tcr, r0, r1, r2, r3);
        printf("Code\r\n%D\r\nStack\r\n%4D\r\n", elr, r0);
    }
    __asm__ __volatile__("1: wfe; b 1b");
}

/*****************************************
 *     Simpleboot loader entry point     *
 *****************************************/
/* actually almost. We come here from _preambule */
void _start(void)
{
    /* initialize everything to zero */
    memset((void*)(uintptr_t)_bss_start, 0, _bss_end - _bss_start);
    fw_init();
    printf("Simpleboot loader, Copyright (c) 2023 bzt, MIT license\r\n");
    /* the emmc driver might print error messages, so call this after we have a console */
    fw_fsinit();

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
    if(!kernel_entry) { printf("ERROR: no kernel entry point\r\n"); goto err; }
    if(verbose > 2) printf("Kernel entry:\r\n%4D", kernel_entry);

    switch(kernel_mode) {
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
            ::"r"(dtb_base):);
        break;
    }
    printf("ERROR: kernel should not have returned\r\n");

    /* there's nowhere to return to on BIOS, halt machine */
err:if(bkp) __asm__ __volatile__("1:wfe; b 1b;");
    else while(1) { if(!(*UART0_FR & 0x10)) { bkp++; goto again; } }
}
