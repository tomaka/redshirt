/*
 * example/linux.c
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
 * @brief An example Linux/x86 Boot Protocol compliant kernel for the Simpleboot loader
 * https://www.kernel.org/doc/html/latest/arch/x86/boot.html
 *
 * This is a very minimal "kernel" that just dumps the boot_params to the serial console.
 */

#include <simpleboot.h>
/* get the Linux structs */
#include "../src/loader.h"

void printf(char *fmt, ...);

/***************************************************************************
 * Linux has no concept of a header with an entry point field, so we must  *
 * keep fixed positions: setup_header must be at 0x1f1 and _start at 0x600 *
 ***************************************************************************/
uint8_t __attribute__((section(".text"))) __attribute__ ((aligned (1)))
    padding1[0x1f1] = { 0 };
linux_boot_t __attribute__((section(".text"))) __attribute__ ((aligned (1))) setup_header = {
.setup_sects = 1,
.boot_flag = 0xAA55,
.jump = 0x6AEB,
.header = 0x53726448, /* "HdrS" */
.version = 0x20c,
.pref_address = 0x100000,
.init_size = 4900-1024
};
uint8_t __attribute__((section(".text"))) __attribute__ ((aligned (1)))
    padding2[0x600 - 0x1f1 - sizeof(setup_header)] = { 0 };

/*****************************************
 *          kernel entry point           *
 *****************************************/
void _start(uint32_t dummy, linux_boot_params_t *bp)
{
    uint32_t i;

    printf("\r\nstruct boot_params at %x\r\n{\r\n", bp);
    printf(".screen_info.lfb_width %d\r\n", bp->lfb_width);
    printf(".screen_info.lfb_height %d\r\n", bp->lfb_height);
    printf(".screen_info.lfb_depth %d\r\n", bp->lfb_depth);
    printf(".screen_info.lfb_base %x\r\n", bp->lfb_base);
    printf(".screen_info.lfb_size %d\r\n", bp->lfb_size);
    printf(".screen_info.lfb_linelength %d\r\n", bp->lfb_linelength);
    printf(".screen_info.red_size %d\r\n", bp->red_size);
    printf(".screen_info.red_pos %d\r\n", bp->red_pos);
    printf(".screen_info.green_size %d\r\n", bp->green_size);
    printf(".screen_info.green_pos %d\r\n", bp->green_pos);
    printf(".screen_info.blue_size %d\r\n", bp->blue_size);
    printf(".screen_info.blue_pos %d\r\n", bp->blue_pos);
    printf(".acpi_rsdp_addr %x\r\n", bp->acpi_rsdp_addr);
    printf(".e820_entries %d\r\n", bp->e820_entries);
    for(i = 0; i < bp->e820_entries && i < E820_MAX_ENTRIES_ZEROPAGE; i++)
        printf(".e820_table[%d] = { .base %012x .size %d .type %x }\r\n", i,
            bp->e820_table[i].addr, bp->e820_table[i].size, bp->e820_table[i].type);
    printf(".hdr.setup_sects %d\r\n", bp->hdr.setup_sects);
    printf(".hdr.version %04x\r\n", bp->hdr.version);
    printf(".hdr.type_of_loader %d\r\n", bp->hdr.type_of_loader);
    printf(".hdr.loadflags %x\r\n", bp->hdr.loadflags);
    printf(".hdr.code32_start %x\r\n", bp->hdr.code32_start);
    printf(".hdr.ramdisk_image %x\r\n", bp->hdr.ramdisk_image);
    printf(".hdr.ramdisk_size %d\r\n", bp->hdr.ramdisk_size);
    printf(".hdr.cmd_line_ptr %x '%s'\r\n", bp->hdr.cmd_line_ptr, (char*)(uintptr_t)bp->hdr.cmd_line_ptr);
    printf(".hdr.pref_address %x\r\n", (uint32_t)bp->hdr.pref_address);
    printf(".hdr.init_size %d\r\n", bp->hdr.init_size);
    printf("}\r\n\r\n");

    /* there's nowhere to return to, halt machine */
    __asm__ __volatile__("1: cli; hlt; jmp 1b");
}

/**
 * Display (extremely minimal) formated message on serial
 */
void printf(char *fmt, ...)
{
    __builtin_va_list args;
    int64_t arg;
    int len, sign, i;
    char *p, tmpstr[19], n;
    /* macro to put a character on serial console */
#define PUTC(c) __asm__ __volatile__( \
                "xorl %%ebx, %%ebx; movb %0, %%bl;" \
                "movl $10000,%%ecx;" \
                "1:inb %%dx, %%al;pause;" \
                "cmpb $0xff,%%al;je 2f;" \
                "dec %%ecx;jz 2f;" \
                "andb $0x20,%%al;jz 1b;" \
                "subb $5,%%dl;movb %%bl, %%al;outb %%al, %%dx;2:" \
                ::"a"(c),"d"(0x3fd): "rbx", "rcx");
    /* parse format and print */
    __builtin_va_start(args, fmt);
    arg = 0;
    while(*fmt) {
        if(*fmt == '%') {
            fmt++;
            if(*fmt == '%') goto put;
            len=0; while(*fmt >= '0' && *fmt <= '9') { len *= 10; len += *fmt - '0'; fmt++; }
            if(*fmt == 'd') {
                arg = __builtin_va_arg(args, int64_t);
                sign = 0; if((int)arg < 0) { arg = -arg; sign++; }
                i = 18; tmpstr[i] = 0;
                do { tmpstr[--i] = '0' + (arg % 10); arg /= 10; } while(arg != 0 && i > 0);
                if(sign) tmpstr[--i] = '-';
                if(len > 0 && len < 18) { while(i > 18 - len) tmpstr[--i] = ' '; }
                p = &tmpstr[i];
                goto putstring;
            } else
            if(*fmt == 'x') {
                arg = __builtin_va_arg(args, int64_t);
                i = 16; tmpstr[i] = 0;
                do { n = arg & 0xf; tmpstr[--i] = n + (n > 9 ? 0x37 : 0x30); arg >>= 4; } while(arg != 0 && i > 0);
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
