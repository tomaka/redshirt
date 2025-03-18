/*
 * src/simpleboot.c
 * https://gitlab.com/bztsrc/simpleboot
 *
 * Copyright (C) 2023 bzt, MIT license
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
 * @brief The bootable disk image generator utility
 *
 * Since this tool is run by users in a hosted environment, and not during boot
 * like the rest, so for portability it has both POSIX and WIN32 bindings. Mostly
 * tested on Linux though, the mingw version might have bugs.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <time.h>
#include <dirent.h>
#include <sys/stat.h>
#include "loader.h"
#include "data.h"
#ifdef __WIN32__
#include <windows.h>
#endif
#ifndef PATH_MAX
#define PATH_MAX 4096
#endif

#define FIRST_PARTITION 2048        /* in sectors, 1M alignment */
#define GPT_SECTORS       62        /* must be smaller than than FIRST_PARTITION - 2 and 16 * 2048 (ISO9660 start) */

const char sbver[] = "1.0.0";       /* tool's version (loaders have different versions) */

guid_t dguid, pguid, ptguid[100], puguid[100];
int64_t disksize = 35, partsize = 33, psize[100] = { 0 }, imgsize, totsize = 0, fat_numclu, fat_freeclu, read_size;
uint8_t *img = NULL, img_tail[(GPT_SECTORS+1)<<9], *fs_base, *cluster = NULL, *pdata = NULL;
uint32_t *fat_fat32 = NULL, fat_nextcluster, ts_x86, ts_rpi;
uint64_t boot_lba = 0, data_lba = 0;
esp_bpb_t *bpb;
void *f;
char *in = NULL, *out = NULL;
char *kernel = NULL, kernelfree = 0, bkpkrnl = 0, bkpfb = 0, bkplogo = 0, full[PATH_MAX], *pfile[100] = { 0 };
int verbose = 0, skipbytes = 0, fat_bpc, fat_spf, num_mod = 0, num_bkp = 0, smp = 0, bkpsmp = 0, nump = 0;
int fb_w = 800, fb_h = 600, fb_bpp = 32;
struct tm *fat_ts;
struct stat dev_st;
#ifdef __WIN32__
WCHAR szFile[PATH_MAX];
#else
char *realpath(const char*, char*);
int fdatasync(int);
#endif
int dev_read(void *f, uint64_t off, void *buf, uint32_t size);
int dev_write(void *f, uint64_t off, void *buf, uint32_t size);

/**
 * Read a file entirely into memory
 */
unsigned char* readfileall(char *file, int chkonly)
{
#ifdef __WIN32__
    HANDLE f;
    DWORD r, t;
    int i;
#else
    FILE *f;
#endif
    unsigned char *data = NULL;

    read_size = 0;
    if(!file || !*file) return NULL;
    data = (unsigned char*)strrchr(file, '/');
    if(data && !*(data + 1)) return NULL;
    data = NULL;
#ifdef __WIN32__
    memset(&szFile, 0, sizeof(szFile));
    MultiByteToWideChar(CP_UTF8, 0, file, -1, szFile, PATH_MAX);
    for(i = 0; szFile[i]; i++) if(szFile[i] == L'/') szFile[i] = L'\\';
    f = CreateFileW(szFile, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL);
    if(f != INVALID_HANDLE_VALUE) {
        r = GetFileSize(f, NULL);
        read_size = (int64_t)r;
        if(!chkonly) {
            data = (unsigned char*)malloc(read_size + 1);
            if(!data) { fprintf(stderr, "%ssimpleboot: unable to allocate memory\r\n", verbose ? "\n" : ""); exit(1); }
            memset(data, 0, read_size + 1);
            if(!ReadFile(f, data, r, &t, NULL)) t = 0;
            read_size = (int64_t)t;
        }
        CloseHandle(f);
    }
#else
    f = fopen(file, "rb");
    if(f) {
        fseek(f, 0L, SEEK_END);
        read_size = (int64_t)ftell(f);
        fseek(f, 0L, SEEK_SET);
        if(!chkonly) {
            data = (unsigned char*)malloc(read_size + 1);
            if(!data) { fprintf(stderr, "%ssimpleboot: unable to allocate memory\r\n", verbose ? "\n" : ""); exit(1); }
            memset(data, 0, read_size + 1);
            read_size = (int64_t)fread(data, 1, read_size, f);
        }
        fclose(f);
    }
#endif
    if(!read_size && data) { free(data); data = NULL; }
    return data;
}

/*** gzip deflate ***/

#define HASH_BITS 12
#define HASH_SIZE (1<<HASH_BITS)
#define MIN_MATCH 3
#define MAX_MATCH 258
#define MAX_OFFSET 32768
#define TO_Lc(x, y) x - 3, y - 3
#define FROM_Lc(x) (x + 3)

typedef struct { uint32_t b, c, t; uint8_t *s, *d; uint16_t e[16], f[288], g[16], h[288]; } zlib_z_t;
typedef struct { uint8_t e; uint8_t min, max; } zlib_c_t;
typedef struct { uint8_t c, e; uint16_t min, max; } zlib_d_t;
zlib_z_t d;
uint8_t *zlib_h[HASH_SIZE];
static const uint8_t mb[256] = {0x00,0x80,0x40,0xc0,0x20,0xa0,0x60,0xe0,0x10,0x90,0x50,0xd0,0x30,0xb0,0x70,0xf0,0x08,0x88,0x48,0xc8,
  0x28,0xa8,0x68,0xe8,0x18,0x98,0x58,0xd8,0x38,0xb8,0x78,0xf8,0x04,0x84,0x44,0xc4,0x24,0xa4,0x64,0xe4,0x14,0x94,0x54,0xd4,0x34,0xb4,
  0x74,0xf4,0x0c,0x8c,0x4c,0xcc,0x2c,0xac,0x6c,0xec,0x1c,0x9c,0x5c,0xdc,0x3c,0xbc,0x7c,0xfc,0x02,0x82,0x42,0xc2,0x22,0xa2,0x62,0xe2,
  0x12,0x92,0x52,0xd2,0x32,0xb2,0x72,0xf2,0x0a,0x8a,0x4a,0xca,0x2a,0xaa,0x6a,0xea,0x1a,0x9a,0x5a,0xda,0x3a,0xba,0x7a,0xfa,0x06,0x86,
  0x46,0xc6,0x26,0xa6,0x66,0xe6,0x16,0x96,0x56,0xd6,0x36,0xb6,0x76,0xf6,0x0e,0x8e,0x4e,0xce,0x2e,0xae,0x6e,0xee,0x1e,0x9e,0x5e,0xde,
  0x3e,0xbe,0x7e,0xfe,0x01,0x81,0x41,0xc1,0x21,0xa1,0x61,0xe1,0x11,0x91,0x51,0xd1,0x31,0xb1,0x71,0xf1,0x09,0x89,0x49,0xc9,0x29,0xa9,
  0x69,0xe9,0x19,0x99,0x59,0xd9,0x39,0xb9,0x79,0xf9,0x05,0x85,0x45,0xc5,0x25,0xa5,0x65,0xe5,0x15,0x95,0x55,0xd5,0x35,0xb5,0x75,0xf5,
  0x0d,0x8d,0x4d,0xcd,0x2d,0xad,0x6d,0xed,0x1d,0x9d,0x5d,0xdd,0x3d,0xbd,0x7d,0xfd,0x03,0x83,0x43,0xc3,0x23,0xa3,0x63,0xe3,0x13,0x93,
  0x53,0xd3,0x33,0xb3,0x73,0xf3,0x0b,0x8b,0x4b,0xcb,0x2b,0xab,0x6b,0xeb,0x1b,0x9b,0x5b,0xdb,0x3b,0xbb,0x7b,0xfb,0x07,0x87,0x47,0xc7,
  0x27,0xa7,0x67,0xe7,0x17,0x97,0x57,0xd7,0x37,0xb7,0x77,0xf7,0x0f,0x8f,0x4f,0xcf,0x2f,0xaf,0x6f,0xef,0x1f,0x9f,0x5f,0xdf,0x3f,0xbf,
  0x7f,0xff};
static const zlib_c_t lc[] = { {0,TO_Lc(3,3)},{0,TO_Lc(4,4)},{0,TO_Lc(5,5)},{0,TO_Lc(6,6)},{0,TO_Lc(7,7)},{0,TO_Lc(8,8)},
  {0,TO_Lc(9,9)},{0,TO_Lc(10,10)},{1,TO_Lc(11,12)},{1,TO_Lc(13,14)},{1,TO_Lc(15,16)},{1,TO_Lc(17,18)},{2,TO_Lc(19,22)},
  {2,TO_Lc(23,26)},{2,TO_Lc(27,30)},{2,TO_Lc(31,34)},{3,TO_Lc(35,42)},{3,TO_Lc(43,50)},{3,TO_Lc(51,58)},{3,TO_Lc(59,66)},
  {4,TO_Lc(67,82)},{4,TO_Lc(83,98)},{4,TO_Lc(99,114)},{4,TO_Lc(115,130)},{5,TO_Lc(131,162)},{5,TO_Lc(163,194)},{5,TO_Lc(195,226)},
  {5,TO_Lc(227,257)},{0,TO_Lc(258,258)}};
static const zlib_d_t dc[] = { {0,0,1,1},{1,0,2,2},{2,0,3,3},{3,0,4,4},{4,1,5,6},{5,1,7,8},{6,2,9,12},{7,2,13,16},{8,3,17,24},
  {9,3,25,32},{10,4,33,48},{11,4,49,64},{12,5,65,96},{13,5,97,128},{14,6,129,192},{15,6,193,256},{16,7,257,384},{17,7,385,512},
  {18,8,513,768},{19,8,769,1024},{20,9,1025,1536},{21,9,1537,2048},{22,10,2049,3072},{23,10,3073,4096},{24,11,4097,6144},
  {25,11,6145,8192},{26,12,8193,12288},{27,12,12289,16384},{28,13,16385,24576},{29,13,24577,32768}};
void zlib_o(uint32_t b, int n) { d.b|=b<<d.c; d.c+=n; while(d.c >= 8){ d.d[d.t++] = (uint8_t)(d.b & 0xFF); d.b >>= 8; d.c -= 8; } }

uint32_t compressBound(uint32_t len) { return len + (len >> 12) + (len >> 14) + (len >> 25) + 13 + 18; }
uint32_t compress(uint8_t *in, uint8_t *out, uint32_t siz)
{
    const zlib_d_t *r;
    const zlib_c_t *l;
    const uint8_t *t = in + siz - MIN_MATCH;
    const uint8_t *q;
    uint8_t **b, *s, c;
    int i, j, k, m, n, o, p, h;

    memset(&d, 0, sizeof(d));
    d.d = out + 2; out[0] = 0x78; out[1] = 0xDA;
    zlib_o(1, 1); zlib_o(1, 2);
    while(in < t) {
        h = (in[0] << 16) | (in[1] << 8) | in[2]; h = ((h >> (3*8 - HASH_BITS)) - h) & (HASH_SIZE - 1);
        b = &zlib_h[h & (HASH_SIZE - 1)]; s = *b; *b = in;
        if(s && in > s && (in - s) <= MAX_OFFSET && !memcmp(in, s, MIN_MATCH)) {
            for(in += MIN_MATCH, q = s + MIN_MATCH, o = MIN_MATCH; *in == *q && o < MAX_MATCH && in < t; in++, q++, o++);
            p = in - o - s;
            while(o > 0) {
                m = (o > 260 ? 258 : o <= 258 ? o : o - 3); o -= m; i = -1; j = sizeof(lc) / sizeof(*lc);
                while(1) {
                    k = (j + i) / 2;
                    if(m < FROM_Lc(lc[k].min)) j = k; else if(m > FROM_Lc(lc[k].max)) i = k; else { l = &lc[k]; break; }
                }
                n = l - lc + 257;
                if(n <= 279) zlib_o(mb[(n - 256) * 2], 7); else zlib_o(mb[0xc0 - 280 + n], 8);
                if (l->e) zlib_o(m - FROM_Lc(l->min), l->e);
                i = -1; j = sizeof(dc) / sizeof(*dc);
                while(1) {
                    k = (j + i) / 2;
                    if(p < dc[k].min) j = k; else if (p > dc[k].max) i = k; else { r = &dc[k]; break; }
                }
                zlib_o(mb[r->c * 8], 5);
                if(r->e) zlib_o(p - r->min, r->e);
            }
        } else { c = *in++; if(c <= 143) zlib_o(mb[0x30 + c], 8); else zlib_o(1 + 2 * mb[0x90 - 144 + c], 9); }
    }
    t += MIN_MATCH;
    while(in < t) { c = *in++; if(c <= 143) zlib_o(mb[0x30 + c], 8); else zlib_o(1 + 2 * mb[0x90 - 144 + c], 9); }
    zlib_o(0, 7); zlib_o(0, 7);
    return d.t + 2;
}

/**
 * CRC calculation with precalculated lookup table
 */
uint32_t crc32_lookup[256]={
    0x00000000, 0x77073096, 0xee0e612c, 0x990951ba, 0x076dc419, 0x706af48f, 0xe963a535, 0x9e6495a3, 0x0edb8832,
    0x79dcb8a4, 0xe0d5e91e, 0x97d2d988, 0x09b64c2b, 0x7eb17cbd, 0xe7b82d07, 0x90bf1d91, 0x1db71064, 0x6ab020f2,
    0xf3b97148, 0x84be41de, 0x1adad47d, 0x6ddde4eb, 0xf4d4b551, 0x83d385c7, 0x136c9856, 0x646ba8c0, 0xfd62f97a,
    0x8a65c9ec, 0x14015c4f, 0x63066cd9, 0xfa0f3d63, 0x8d080df5, 0x3b6e20c8, 0x4c69105e, 0xd56041e4, 0xa2677172,
    0x3c03e4d1, 0x4b04d447, 0xd20d85fd, 0xa50ab56b, 0x35b5a8fa, 0x42b2986c, 0xdbbbc9d6, 0xacbcf940, 0x32d86ce3,
    0x45df5c75, 0xdcd60dcf, 0xabd13d59, 0x26d930ac, 0x51de003a, 0xc8d75180, 0xbfd06116, 0x21b4f4b5, 0x56b3c423,
    0xcfba9599, 0xb8bda50f, 0x2802b89e, 0x5f058808, 0xc60cd9b2, 0xb10be924, 0x2f6f7c87, 0x58684c11, 0xc1611dab,
    0xb6662d3d, 0x76dc4190, 0x01db7106, 0x98d220bc, 0xefd5102a, 0x71b18589, 0x06b6b51f, 0x9fbfe4a5, 0xe8b8d433,
    0x7807c9a2, 0x0f00f934, 0x9609a88e, 0xe10e9818, 0x7f6a0dbb, 0x086d3d2d, 0x91646c97, 0xe6635c01, 0x6b6b51f4,
    0x1c6c6162, 0x856530d8, 0xf262004e, 0x6c0695ed, 0x1b01a57b, 0x8208f4c1, 0xf50fc457, 0x65b0d9c6, 0x12b7e950,
    0x8bbeb8ea, 0xfcb9887c, 0x62dd1ddf, 0x15da2d49, 0x8cd37cf3, 0xfbd44c65, 0x4db26158, 0x3ab551ce, 0xa3bc0074,
    0xd4bb30e2, 0x4adfa541, 0x3dd895d7, 0xa4d1c46d, 0xd3d6f4fb, 0x4369e96a, 0x346ed9fc, 0xad678846, 0xda60b8d0,
    0x44042d73, 0x33031de5, 0xaa0a4c5f, 0xdd0d7cc9, 0x5005713c, 0x270241aa, 0xbe0b1010, 0xc90c2086, 0x5768b525,
    0x206f85b3, 0xb966d409, 0xce61e49f, 0x5edef90e, 0x29d9c998, 0xb0d09822, 0xc7d7a8b4, 0x59b33d17, 0x2eb40d81,
    0xb7bd5c3b, 0xc0ba6cad, 0xedb88320, 0x9abfb3b6, 0x03b6e20c, 0x74b1d29a, 0xead54739, 0x9dd277af, 0x04db2615,
    0x73dc1683, 0xe3630b12, 0x94643b84, 0x0d6d6a3e, 0x7a6a5aa8, 0xe40ecf0b, 0x9309ff9d, 0x0a00ae27, 0x7d079eb1,
    0xf00f9344, 0x8708a3d2, 0x1e01f268, 0x6906c2fe, 0xf762575d, 0x806567cb, 0x196c3671, 0x6e6b06e7, 0xfed41b76,
    0x89d32be0, 0x10da7a5a, 0x67dd4acc, 0xf9b9df6f, 0x8ebeeff9, 0x17b7be43, 0x60b08ed5, 0xd6d6a3e8, 0xa1d1937e,
    0x38d8c2c4, 0x4fdff252, 0xd1bb67f1, 0xa6bc5767, 0x3fb506dd, 0x48b2364b, 0xd80d2bda, 0xaf0a1b4c, 0x36034af6,
    0x41047a60, 0xdf60efc3, 0xa867df55, 0x316e8eef, 0x4669be79, 0xcb61b38c, 0xbc66831a, 0x256fd2a0, 0x5268e236,
    0xcc0c7795, 0xbb0b4703, 0x220216b9, 0x5505262f, 0xc5ba3bbe, 0xb2bd0b28, 0x2bb45a92, 0x5cb36a04, 0xc2d7ffa7,
    0xb5d0cf31, 0x2cd99e8b, 0x5bdeae1d, 0x9b64c2b0, 0xec63f226, 0x756aa39c, 0x026d930a, 0x9c0906a9, 0xeb0e363f,
    0x72076785, 0x05005713, 0x95bf4a82, 0xe2b87a14, 0x7bb12bae, 0x0cb61b38, 0x92d28e9b, 0xe5d5be0d, 0x7cdcefb7,
    0x0bdbdf21, 0x86d3d2d4, 0xf1d4e242, 0x68ddb3f8, 0x1fda836e, 0x81be16cd, 0xf6b9265b, 0x6fb077e1, 0x18b74777,
    0x88085ae6, 0xff0f6a70, 0x66063bca, 0x11010b5c, 0x8f659eff, 0xf862ae69, 0x616bffd3, 0x166ccf45, 0xa00ae278,
    0xd70dd2ee, 0x4e048354, 0x3903b3c2, 0xa7672661, 0xd06016f7, 0x4969474d, 0x3e6e77db, 0xaed16a4a, 0xd9d65adc,
    0x40df0b66, 0x37d83bf0, 0xa9bcae53, 0xdebb9ec5, 0x47b2cf7f, 0x30b5ffe9, 0xbdbdf21c, 0xcabac28a, 0x53b39330,
    0x24b4a3a6, 0xbad03605, 0xcdd70693, 0x54de5729, 0x23d967bf, 0xb3667a2e, 0xc4614ab8, 0x5d681b02, 0x2a6f2b94,
    0xb40bbe37, 0xc30c8ea1, 0x5a05df1b, 0x2d02ef8d};
uint32_t crc32_calc(unsigned char *start,int length)
{
    uint32_t crc32_val=0xffffffff;
    while(length--) crc32_val=(crc32_val>>8)^crc32_lookup[(crc32_val&0xff)^(unsigned char)*start++];
    crc32_val^=0xffffffff;
    return crc32_val;
}

/**
 * Convert LBA to CHS
 */
void chs(uint32_t lba, void *chs)
{
    /* we don't have a real geometry, so assume 16 heads and 63 sectors */
    uint64_t c, h = 16, s = 63;
    c = lba / (h*s); lba %= h*s;
    *((uint8_t*)(chs+3)) = c & 0xff;
    *((uint8_t*)(chs+2)) = ((c >> 2) & 0xc0) | ((lba % s) + 1);
    *((uint8_t*)(chs+1)) = lba / s;
}

/**
 * Add GUID Partitioning Table
 */
void gpt_create(void)
{
    uint64_t lba;
    guid_t efiguid = EFI_PART_TYPE_EFI_SYSTEM_PART_GUID;
    gpt_header_t *hdr = (gpt_header_t *)(img + 512);
    gpt_entry_t *entry = (gpt_entry_t *)(img + 1024);
    int i, j;
    char *name = "EFI System Partition", *part = "Partition ";

    /* Protective Master Boot Record (we don't really need nor use this, just for backward compatibility) */
    memcpy(img+0x1B8, &dguid.Data1, 4);             /* WinNT disk id */
    /* MBR, EFI System Partition / boot partition. */
    img[0x1C0-2]=0x80;                              /* bootable flag */
    chs(FIRST_PARTITION, img+0x1BE);                /* start CHS */
    img[0x1C0+2]=0xC;                               /* type, LBA FAT32 (0xC) */
    chs(FIRST_PARTITION+partsize/512-1, img+0x1C2); /* end CHS */
    *((uint32_t*)(img+0x1C0+6)) = FIRST_PARTITION;  /* start LBA */
    *((uint32_t*)(img+0x1C0+10)) = partsize/512;    /* number of sectors */
    /* MBR, protective img entry */
    chs(1, img+0x1CE);                              /* start CHS */
    img[0x1D0+2]=0xEE;                              /* type */
    chs(GPT_SECTORS-1, img+0x1D2);                  /* end CHS */
    *((uint32_t*)(img+0x1D0+6)) = 1;                /* start LBA */
    *((uint32_t*)(img+0x1D0+10)) = GPT_SECTORS-1;   /* number of sectors */

    /* GPT header */
    memcpy(&hdr->Signature, EFI_PTAB_HEADER_ID, 8);
    hdr->Revision = 0x10000;
    hdr->HeaderSize = 92;
    hdr->MyLBA = 1;
    hdr->AlternateLBA = disksize / 512 - 1;
    hdr->FirstUsableLBA = FIRST_PARTITION;
    hdr->LastUsableLBA = disksize / 512 - GPT_SECTORS - 1;
    memcpy(&hdr->DiskGUID, &dguid, sizeof(guid_t));
    hdr->PartitionEntryLBA = 2;
    hdr->NumberOfPartitionEntries = GPT_SECTORS * 512 / sizeof(gpt_entry_t);
    hdr->SizeOfPartitionEntry = sizeof(gpt_entry_t);

    /* GPT, EFI System Partition (ESP) */
    memcpy(&entry->PartitionTypeGUID, &efiguid, sizeof(guid_t));
    memcpy(&entry->UniquePartitionGUID, &pguid, sizeof(guid_t));
    entry->StartingLBA = FIRST_PARTITION;
    entry->EndingLBA = FIRST_PARTITION + (partsize / 512) - 1;
    for(i = 0; name[i]; i++) entry->PartitionName[i] = name[i];
    lba = (FIRST_PARTITION * 512 + partsize + 1024*1024-1) & ~(1024*1024-1);

    for(j = 0; j < nump; j++)
        if(psize[j] > 0) {
            /* GPT, Extra Partition(s) */
            entry++;
            memcpy(&entry->PartitionTypeGUID, &ptguid[j], sizeof(guid_t));
            memcpy(&entry->UniquePartitionGUID, &puguid[j], sizeof(guid_t));
            entry->StartingLBA = lba / 2048;
            lba += (psize[i] + 1024*1024-1) & ~(1024*1024-1);
            entry->EndingLBA = lba / 2048 - 1;
            for(i = 0; part[i]; i++) entry->PartitionName[i] = part[i];
            entry->PartitionName[i++] = (j / 10) + '0';
            entry->PartitionName[i++] = (j % 10) + '0';
        }

    /* calculate checksums */
    hdr->PartitionEntryArrayCRC32 = crc32_calc((unsigned char*)entry, hdr->NumberOfPartitionEntries * hdr->SizeOfPartitionEntry);
    hdr->CRC32 = crc32_calc((unsigned char*)hdr, hdr->HeaderSize);

    /* secondary GPT */
    memcpy(img_tail, img + 1024, GPT_SECTORS * 512);
    hdr = (gpt_header_t*)(img_tail + GPT_SECTORS * 512);
    memcpy(hdr, img + 512, 512);
    hdr->MyLBA = disksize / 512 - 1;
    hdr->AlternateLBA = 1;
    hdr->PartitionEntryLBA = disksize / 512 - GPT_SECTORS - 1;
    hdr->CRC32 = 0; hdr->CRC32 = crc32_calc((unsigned char*)hdr, hdr->HeaderSize);
}

/**
 * Set an integer value both little-endian and big-endian in El Torito records
 */
void setinte(uint32_t val, unsigned char *ptr) {
    uint8_t *v = (uint8_t*)&val;
    ptr[0] = ptr[7] = v[0]; ptr[1] = ptr[6] = v[1]; ptr[2] = ptr[5] = v[2]; ptr[3] = ptr[4] = v[3];
}

/**
 * Add ISO9660 El Torito Boot Catalog for bootable CDROM
 * WARNING: optical discs are obsolete, not a real world use case any more, so dirty hacks ahead
 */
void etbc_create(void)
{
    uint8_t *iso = img + 16 * 2048;
    char isodate[128];
    int i;

    /* How and why this ugly hack works on legacy BIOS:
     * 1. Normally on EFI the firmware locates the ESP in the GPT and parses it to find the 2nd stage loader as a file.
     *    The ESP's VBR is never executed (actually fat_format() does not place any code here at all).
     * 2. On BIOS, the very first sector is loaded and executed. Originally on an IBM PC this MBR loaded and executed
     *    the partition's VBR; however boot_x86.asm directly loads the 2nd stage so the ESP's VBR again, irrelevant.
     * 3. EFI firmware in El Torito mode (see below) locates the ESP in the Boot Catalog and again, interprets it as a
     *    file system, so the ESP's VBR code isn't used in this case either.
     * 4. Now the BIOS firmware in El Torito mode loads the sector where the Boot Catalog points (which must be the
     *    ESP's VBR because of case 3) and directly executes it. This is the only case when the VBR's code matters.
     * So cdemu_x86.asm in this sector simulates a secondary HDD with the CDROM's data, loads the MBR and executes it,
     * and after that everything goes like in case 2, business as usual.
     *
     * Install HDD emulation of CDROM for BIOS code into the ESP VBR */
    memcpy(fs_base, cdemu_x86_bin, sizeof(cdemu_x86_bin));

    /* from the UEFI spec section 12.3.2.1 ISO-9660 and El Torito
      "...A Platform ID of 0xEF indicates an EFI System Partition. The Platform ID is in either the Section
      Header Entry or the Validation Entry of the Booting Catalog as defined by the “El Torito”
      specification. EFI differs from “El Torito” “no emulation” mode in that it does not load the “no
      emulation” image into memory and jump to it. EFI interprets the “no emulation” image as an EFI
      system partition."
     * so we must record the ESP in the Boot Catalog, that's how UEFI locates it */

    sprintf((char*)&isodate, "%04d%02d%02d%02d%02d%02d00",
        fat_ts->tm_year+1900,fat_ts->tm_mon+1,fat_ts->tm_mday,fat_ts->tm_hour,fat_ts->tm_min,fat_ts->tm_sec);

    /* 16th sector: Primary Volume Descriptor */
    iso[0]=1;   /* Header ID */
    memcpy(&iso[1], "CD001", 5);
    iso[6]=1;   /* version */
    for(i=8;i<72;i++) iso[i]=' ';
    memcpy(&iso[40], "BOOT CDROM", 10);   /* Volume Identifier */
    setinte((disksize+2047)/2048, &iso[80]);
    iso[120]=iso[123]=1;        /* Volume Set Size */
    iso[124]=iso[127]=1;        /* Volume Sequence Number */
    iso[129]=iso[130]=8;        /* logical blocksize (0x800) */
    iso[156]=0x22;              /* root directory recordsize */
    setinte(20, &iso[158]);     /* root directory LBA */
    setinte(2048, &iso[166]);   /* root directory size */
    iso[174]=fat_ts->tm_year;   /* root directory create date */
    iso[175]=fat_ts->tm_mon+1;
    iso[176]=fat_ts->tm_mday;
    iso[177]=fat_ts->tm_hour;
    iso[178]=fat_ts->tm_min;
    iso[179]=fat_ts->tm_sec;
    iso[180]=0;                 /* timezone UTC (GMT) */
    iso[181]=2;                 /* root directory flags (0=hidden,1=directory) */
    iso[184]=iso[187]=1;        /* root directory number */
    iso[188]=1;                 /* root directory filename length */
    for(i=190;i<813;i++) iso[i]=' ';    /* Volume data */
    memcpy(&iso[318], "SIMPLEBOOT <HTTPS://CODEBERG.ORG/BZT/SIMPLEBOOT>", 48);
    memcpy(&iso[446], "SIMPLEBOOT", 10);
    memcpy(&iso[574], "BOOT CDROM", 10);
    for(i=702;i<813;i++) iso[i]=' ';    /* file descriptors */
    memcpy(&iso[813], &isodate, 16);    /* volume create date */
    memcpy(&iso[830], &isodate, 16);    /* volume modify date */
    for(i=847;i<863;i++) iso[i]='0';    /* volume expiration date */
    for(i=864;i<880;i++) iso[i]='0';    /* volume shown date */
    iso[881]=1;                         /* filestructure version */
    for(i=883;i<1395;i++) iso[i]=' ';   /* file descriptors */
    /* 17th sector: Boot Record Descriptor */
    iso[2048]=0;    /* Header ID */
    memcpy(&iso[2049], "CD001", 5);
    iso[2054]=1;    /* version */
    memcpy(&iso[2055], "EL TORITO SPECIFICATION", 23);
    setinte(19, &iso[2048+71]);         /* Boot Catalog LBA */
    /* 18th sector: Volume Descritor Terminator */
    iso[4096]=0xFF; /* Header ID */
    memcpy(&iso[4097], "CD001", 5);
    iso[4102]=1;    /* version */
    /* 19th sector: Boot Catalog */
    iso[6144]=1;    /* Header ID, Validation Entry */
    iso[6145]=0;    /* Platform 80x86 */
    iso[6172]=0xaa; /* magic bytes */
    iso[6173]=0x55;
    iso[6174]=0x55;
    iso[6175]=0xaa;
    iso[6176]=0x88; /* Bootable, Initial/Default Entry */
    iso[6182]=4;    /* Sector Count */
    *((uint32_t*)(iso + 6184)) = FIRST_PARTITION/4; /* ESP Start LBA */
    iso[6208]=0x91; /* Header ID, Final Section Header Entry */
    iso[6209]=0xEF; /* Platform EFI */
    iso[6210]=1;    /* Number of entries */
    iso[6240]=0x88; /* Bootable, Section Entry */
    *((uint16_t*)(iso + 6246)) = partsize/512;      /* Sector Count */
    *((uint32_t*)(iso + 6248)) = FIRST_PARTITION/4; /* ESP Start LBA */
    /* 20th sector: Root Directory */
    /* . */
    iso[8192]=0x21 + 1;          /* recordsize */
    setinte(20, &iso[8194]);     /* LBA */
    setinte(2048, &iso[8202]);   /* size */
    iso[8210]=fat_ts->tm_year;   /* date */
    iso[8211]=fat_ts->tm_mon+1;
    iso[8212]=fat_ts->tm_mday;
    iso[8213]=fat_ts->tm_hour;
    iso[8214]=fat_ts->tm_min;
    iso[8215]=fat_ts->tm_sec;
    iso[8216]=0;                 /* timezone UTC (GMT) */
    iso[8217]=2;                 /* flags (0=hidden,1=directory) */
    iso[8220]=iso[8223]=1;       /* serial */
    iso[8224]=1;                 /* filename length */
    iso[8225]=0;                 /* filename '.' */
    /* .. */
    iso[8226]=0x21 + 1;          /* recordsize */
    setinte(20, &iso[8228]);     /* LBA */
    setinte(2048, &iso[8236]);   /* size */
    iso[8244]=fat_ts->tm_year;   /* date */
    iso[8245]=fat_ts->tm_mon+1;
    iso[8246]=fat_ts->tm_mday;
    iso[8247]=fat_ts->tm_hour;
    iso[8248]=fat_ts->tm_min;
    iso[8249]=fat_ts->tm_sec;
    iso[8250]=0;                 /* timezone UTC (GMT) */
    iso[8251]=2;                 /* flags (0=hidden,1=directory) */
    iso[8254]=iso[8257]=1;       /* serial */
    iso[8258]=1;                 /* filename length */
    iso[8259]='\001';            /* filename '..' */
}

/**
 * Create legacy ROM images (use loader_cb.c instead if you can)
 */
void rom_create(char *path)
{
    pcirom_t *pci;
    fb_rom_t *foss;
    FILE *f = NULL;
    char *out = NULL, *name;
    uint8_t buf[512 + 65536], in[512 + 65536];
    int i, c, romsize = 512 + ((sizeof(loader_x86_efi) + 511) & ~511);

    /* get directory from output image path */
    if(!path || !*path || !memcmp(path, "/dev/", 5) || !memcmp(path, "\\\\.\\", 4)) {
        if(!(out = (char*)malloc(32))) goto err;
        name = out;
    } else {
        if(!(out = (char*)malloc(strlen(path) + 32))) goto err;
        strcpy(out, path);
        if((name = strrchr(out, '/'))) name++; else
        if((name = strrchr(out, '\\'))) name++; else name = out;
    }

    /* save legacy BIOS Expansion ROM */
    strcpy(name, "sb_bios.rom");
    memset(buf, 0, romsize);
    memcpy(buf, rombios_x86_bin, sizeof(rombios_x86_bin));
    memcpy(buf + sizeof(rombios_x86_bin), loader_x86_efi, sizeof(loader_x86_efi));
    buf[2] = romsize >> 9;
    for(i = c = 0; i < romsize; i++) { c += buf[i]; } buf[6] = (uint8_t)(256 - c);
    if(!(f = fopen(out, "wb")) || !fwrite(buf, 1, romsize, f)) goto err;
    fclose(f);

    /* save legacy UEFI PCI Option ROM */
    strcpy(name, "sb_uefi.rom");
    memset(buf, 0, romsize);
    memcpy(buf + sizeof(pcirom_t), loader_x86_efi, sizeof(loader_x86_efi));
    pci = (pcirom_t*)buf;
    pci->magic[0] = 0x55; pci->magic[1] = 0xAA;
    pci->InitializationSize = pci->ImageLength = romsize >> 9;
    pci->EfiSignature = 0x0EF1;
    pci->EfiSubsystem = 0xA;
    pci->EfiMachineType = IMAGE_FILE_MACHINE_AMD64;
    pci->EfiImageHeaderOffset = sizeof(pcirom_t);
    pci->PcirOffset = 0x1C;
    memcpy(&pci->Signature, "PCIR", 4);
    pci->VendorId = 0x8086;
    pci->DeviceId = 0x100E;
    pci->Length = 24;
    pci->ClassCode[0] = 0; /* no signed check for PCI_CLASS_VGA */
    pci->CodeType = 0x03;
    pci->Indicator = 0x80;
    for(i = c = 0; i < romsize; i++) { c += buf[i]; } pci->checksum = (uint8_t)(256 - c);
    if(!(f = fopen(out, "wb")) || !fwrite(buf, 1, romsize, f)) goto err;
    fclose(f);

    /* save FOSSBIOS ROM */
    strcpy(name, "sb_foss.rom");
    memset(buf, 0, romsize);
    memcpy(in, romfoss_x86_bin, sizeof(romfoss_x86_bin));
    memcpy(in + sizeof(romfoss_x86_bin), loader_x86_efi, sizeof(loader_x86_efi));
    foss = (fb_rom_t*)buf;
    foss->magic[0] = 0x55; foss->magic[1] = 0xAA;
    foss->fossmagic[0] = 0xF0; foss->fossmagic[1] = 0x55; foss->fossmagic[2] = 0xB1; foss->fossmagic[3] = 0x05;
    foss->arch = 0;     /* x86 */
    foss->wordsize = 8; /* 64-bit */
    foss->type = 1;     /* boot services */
    foss->uncompressed = sizeof(romfoss_x86_bin) + sizeof(loader_x86_efi);
    foss->compressed = compress(in, buf + sizeof(fb_rom_t), foss->uncompressed);
    romsize = (sizeof(fb_rom_t) + foss->compressed + 511) & ~511;
    foss->size = romsize >> 9;
    memcpy(&foss->pcir[4], "Simpleboot\0x86_64", 17);
    foss->crc = crc32_calc(buf, sizeof(fb_rom_t) + foss->compressed);
    if(!(f = fopen(out, "wb")) || !fwrite(buf, 1, romsize, f)) goto err;
    fclose(f);
    free(out);
    return;

err:fprintf(stderr, "simpleboot: unable to save ROM images\r\n");
    if(out) free(out);
    if(f) fclose(f);
}

/**
 * Finish FAT
 */
void fat_finish(void)
{
    int i;

    fat_nextcluster -= 2;
    i = fat_freeclu ? fat_freeclu : ((partsize - (fat_spf*fs_base[0x10]+fs_base[0xE]) * 512)/fat_bpc) - fat_nextcluster;
    fs_base[0x3E8] = i & 0xFF; fs_base[0x3E9] = (i >> 8) & 0xFF;
    fs_base[0x3EA] = (i >> 16) & 0xFF; fs_base[0x3EB] = (i >> 24) & 0xFF;
    fs_base[0x3EC] = fat_nextcluster & 0xFF; fs_base[0x3ED] = (fat_nextcluster >> 8) & 0xFF;
    fs_base[0x3EE] = (fat_nextcluster >> 16) & 0xFF; fs_base[0x3EF] = (fat_nextcluster >> 24) & 0xFF;
    /* copy backup boot sectors */
    memcpy(fs_base + (fs_base[0x32]*512), fs_base, 1024);
}

/**
 * Add a 8.3 FAT directory entry
 */
uint8_t *fat_dirent83(uint8_t *ptr, char *name, int type, uint32_t clu, uint32_t size)
{
    int i;

    for(i = 0; i < 11 && name[i]; i++) ptr[i] = name[i];
    while(i < 11) ptr[i++] = ' ';
    memset(ptr + 11, 0, 21);
    ptr[0xB] = type;
    i = (fat_ts->tm_hour << 11) | (fat_ts->tm_min << 5) | (fat_ts->tm_sec/2);
    ptr[0xE] = ptr[0x16] = i & 0xFF; ptr[0xF] = ptr[0x17] = (i >> 8) & 0xFF;
    i = ((fat_ts->tm_year+1900-1980) << 9) | ((fat_ts->tm_mon+1) << 5) | (fat_ts->tm_mday);
    ptr[0x10] = ptr[0x12] = ptr[0x18] = i & 0xFF; ptr[0x11] = ptr[0x13] = ptr[0x19] = (i >> 8) & 0xFF;
    ptr[0x1A] = clu & 0xFF; ptr[0x1B] = (clu >> 8) & 0xFF;
    ptr[0x14] = (clu >> 16) & 0xFF; ptr[0x15] = (clu >> 24) & 0xFF;
    ptr[0x1C] = size & 0xFF; ptr[0x1D] = (size >> 8) & 0xFF;
    ptr[0x1E] = (size >> 16) & 0xFF; ptr[0x1F] = (size >> 24) & 0xFF;
    return ptr + 32;
}

/**
 * Format the partition to FAT32
 */
void fat_format(void)
{
    int i = FIRST_PARTITION;
    uint32_t *fat_fat32_1, *fat_fat32_2;
    uint8_t *fat_rootdir;

    fat_numclu = partsize / 512; fat_freeclu = 0;
    if(fat_numclu < 67584) { fprintf(stderr, "simpleboot: not enough clusters\r\n"); exit(1); }
    /* make BPB (FAT superblock) */
    memcpy(fs_base + 3, "MSWIN4.1", 8);
    fs_base[0xC] = 2; fs_base[0x10] = 2; fs_base[0x15] = 0xF8; fs_base[0x1FE] = 0x55; fs_base[0x1FF] = 0xAA;
    fs_base[0x18] = 240; fs_base[0x19] = 3; fs_base[0x1A] = 16;
    memcpy(fs_base + 0x1C, &i, 4);
    memcpy(fs_base + 0x20, &fat_numclu, 4);
    fs_base[0xD] = 1; fs_base[0xE] = 8;
    fat_spf = (fat_numclu*4) / 512 + fs_base[0xE];
    fs_base[0x24] = fat_spf & 0xFF; fs_base[0x25] = (fat_spf >> 8) & 0xFF;
    fs_base[0x26] = (fat_spf >> 16) & 0xFF; fs_base[0x27] = (fat_spf >> 24) & 0xFF;
    fs_base[0x2C] = 2; fs_base[0x30] = 1; fs_base[0x32] = 6; fs_base[0x40] = 0x80; fs_base[0x42] = 0x29;
    memcpy(fs_base + 0x43, &pguid, 4);
    memcpy(fs_base + 0x47, "EFI System FAT32   ", 19);
    memcpy(fs_base + 0x200, "RRaA", 4); memcpy(fs_base + 0x3E4, "rrAa", 4);
    for(i = 0; i < 8; i++) fs_base[0x3E8 + i] = 0xFF;
    fs_base[0x3FE] = 0x55; fs_base[0x3FF] = 0xAA;
    fat_bpc = fs_base[0xD] * 512;
    fat_rootdir = fs_base + (fat_spf*fs_base[0x10]+fs_base[0xE]) * 512;
    fat_fat32_1 = (uint32_t*)(&fs_base[fs_base[0xE] * 512]);
    fat_fat32_2 = (uint32_t*)(&fs_base[(fs_base[0xE]+fat_spf) * 512]);
    fat_fat32_1[0] = fat_fat32_2[0] = 0x0FFFFFF8;
    fat_fat32_1[1] = fat_fat32_2[1] = fat_fat32_1[2] = fat_fat32_2[2] = 0x0FFFFFFF;
    fat_nextcluster = 3;
    /* label in root directory */
    fat_dirent83(fat_rootdir, "EFI System ", 8, 0, 0);
    fat_finish();
}

/**
 * Find a free cluster (or more)
 */
uint32_t fat_findclu(uint32_t cnt)
{
    uint32_t i, clu;

    if(cnt < 1) cnt = 1;
    if(fat_freeclu < (int64_t)cnt) {
        fprintf(stderr, "%ssimpleboot: not enough space on the boot partition\r\n", verbose ? "\n" : "");
        return 0;
    }
    for(clu = 3; clu < fat_nextcluster; clu++) {
        for(i = 0; i < cnt && clu + i < fat_nextcluster && !fat_fat32[clu + i]; i++);
        if(i == cnt) break;
    }
    if(clu == fat_nextcluster) fat_nextcluster += cnt;
    fat_freeclu -= cnt;
    for(i = 0; i < cnt; i++) fat_fat32[clu + i] = clu + i + 1;
    fat_fat32[clu + i - 1] = 0x0FFFFFFF;
    return clu;
}

/**
 * Look up or add a directory entry
 * parent: cluster where we need to add
 * add: 0=just check if entry exists, 1=update or add if doesn't exists, 2=update or add, allocate contiguous clusters
 * name: UTF-8 filename
 * isdir: 1 if the entry is a directory
 * *clu: returned cluster
 * flen: file's length
 */
int fat_dirent(uint32_t parent, int add, char *name, int isdir, uint32_t *clu, uint32_t flen)
{
    char ucase[256], fn[256];
    uint16_t uc2[32 * 13 + 1], *u;
    uint32_t i, n, size, last, cnt = 0;
    uint8_t *s, *dir, *ptr, c;

    *clu = -1U;

    /* convert name to uppercase */
    for(i = 0; i < 255 && name[i]; i++)
        ucase[i] = name[i] >= 'a' && name[i] <= 'z' ? name[i] - 'a' + 'A' : name[i];
    ucase[i] = 0;

    /* get the size */
    for(i = parent, size = fat_bpc;
        i != fat_nextcluster && fat_fat32[i] && fat_fat32[i] < 0xFFFFFF8; i = fat_fat32[i], size += fat_bpc);
    if(!(ptr = (uint8_t*)malloc(size + 4 * fat_bpc))) {
        fprintf(stderr, "%ssimpleboot: unable to allocate memory\r\n", verbose ? "\n" : "");
        return 0;
    }
    memset(ptr, 0, size + 4 * fat_bpc);

    /* load the entries */
    i = last = parent; dir = ptr;
    do {
        if(!dev_read(f, (data_lba + i * bpb->spc) << 9, dir, fat_bpc)) {
            free(ptr); fprintf(stderr, "%ssimpleboot: unable to read '%s'\r\n", verbose ? "\n" : "", out); return 0;
        }
        last = i; i = fat_fat32[i]; dir += fat_bpc;
    } while(i && i < fat_nextcluster && i < 0xFFFFFF8);

    /* iterate on entries */
    for(dir = ptr; dir < ptr + size && *dir && (dir < ptr + 64 || *dir != '.'); dir += 32, cnt++) {
        memset(fn, 0, sizeof(fn));
        if(dir[0] == 0xE5) continue;
        if(dir[0xB] == 0xF) {
            /* this is an LFN block */
            memset(uc2, 0, sizeof(uc2));
            n = dir[0] & 0x1F;
            u = uc2 + (n - 1) * 13;
            while(n--) {
                for(i = 0; i < 5; i++) u[i] = dir[i*2+2] << 8 | dir[i*2+1];
                for(i = 0; i < 6; i++) u[i+5] = dir[i*2+0xF] << 8 | dir[i*2+0xE];
                u[11] = dir[0x1D] << 8 | dir[0x1C];
                u[12] = dir[0x1F] << 8 | dir[0x1E];
                u -= 13;
                dir += 32;
            }
            for(s = (uint8_t*)fn, u = uc2; *u && s < (uint8_t*)fn + 255; u++)
                if(*u < 0x80) { *s++ = *u >= 'a' && *u <= 'z' ? *u - 'a' + 'A' : *u; } else
                if(*u < 0x800) { *s++ = ((*u>>6)&0x1F)|0xC0; *s++ = (*u&0x3F)|0x80; } else
                { *s++ = ((*u>>12)&0x0F)|0xE0; *s++ = ((*u>>6)&0x3F)|0x80; *s++ = (*u&0x3F)|0x80; }
            *s = 0;
        } else {
            /* use 8.3 name otherwise */
            for(i = 0, s = (uint8_t*)fn; i < 8 && dir[i] != ' '; i++, s++)
                *s = dir[i] >= 'a' && dir[i] <= 'z' ? dir[i] - 'a' + 'A' : dir[i];
            if(dir[8] != ' ') {
                *s++ = '.';
                for(i = 8; i < 11 && dir[i] != ' '; i++, s++)
                    *s = dir[i] >= 'a' && dir[i] <= 'z' ? dir[i] - 'a' + 'A' : dir[i];
            }
            *s = 0;
        }
        if(!strcmp(fn, ucase)) {
            *clu = (dir[0x15] << 24) | (dir[0x14] << 16) | (dir[0x1B] << 8) | dir[0x1A];
            /* if it's a file, delete it */
            if(!dir[0xB] && add) {
                for(i = *clu; i && i < fat_nextcluster && i < 0xFFFFFF8;) {
                    n = fat_fat32[i]; fat_fat32[i] = 0; i = n; fat_freeclu++;
                }
                if(*clu) fat_freeclu--;
                if(!flen) {
                    *clu = 0; dir[0x1A] = dir[0x1B] = dir[0x14] = dir[0x15] = 0;
                } else {
                    /* if we need contiguous allocation, update the cluster too */
                    if(add == 2) {
                        if(*clu && *clu < fat_nextcluster && *clu < 0xFFFFFF8) { fat_fat32[*clu] = 0; fat_freeclu++; }
                        *clu = fat_findclu((flen + fat_bpc - 1) / fat_bpc);
                        dir[0x1A] = *clu & 0xFF; dir[0x1B] = (*clu >> 8) & 0xFF;
                        dir[0x14] = (*clu >> 16) & 0xFF; dir[0x15] = (*clu >> 24) & 0xFF;
                    } else
                    if(*clu && *clu < fat_nextcluster && *clu < 0xFFFFFF8) fat_fat32[*clu] = 0xFFFFFFF;
                }
                dir[0x1C] = flen & 0xFF; dir[0x1D] = (flen >> 8) & 0xFF;
                dir[0x1E] = (flen >> 16) & 0xFF; dir[0x1F] = (flen >> 24) & 0xFF;
                /* write out modified directory entry */
                for(s = ptr, i = parent; s + fat_bpc < dir; s += fat_bpc) i = fat_fat32[i];
                if(!dev_write(f, ((data_lba + i * bpb->spc) << 9), s, fat_bpc)) {
                    fprintf(stderr, "%ssimpleboot: unable to write '%s'\r\n", verbose ? "\n" : "", out);
                }
            }
            break;
        }
    }

    /* if not found, add the directory entry */
    if(*clu == -1U && add) {
        for(i = n = 0; ucase[i] && ucase[i] != '.'; i++);
        if(ucase[i] == '.') for(; ucase[i + n + 1]; n++);
        if(i <= 8 && n <= 3 && !strcmp(name, ucase)) {
            /* use the 8.3 name if possible */
            memset(fn, ' ', 11);
            for(i = n = 0; ucase[i]; i++) { if(ucase[i] == '.') n = 8; else fn[n++] = ucase[i]; }
            n = 0;
        } else {
            memset(uc2, 0, sizeof(uc2));
            for(i = 0, u = uc2, s = (uint8_t*)name; *s; i++, u++) {
                if((*s & 128) != 0) {
                    if((*s & 32) == 0) { *u = ((*s & 0x1F)<<6)|(*(s+1) & 0x3F); s += 2; } else
                    if((*s & 16) == 0) { *u = ((*s & 0xF)<<12)|((*(s+1) & 0x3F)<<6)|(*(s+2) & 0x3F); s += 3; }
                } else
                    *u = *s++;
            }
            i = (i + 12) / 13;
            /* don't convert "Microsoft" to "MICROS~1   ", that's patented... */
            sprintf(fn, "~%07xLFN", cnt++);
            for(c = 0, n = 0; n < 11; n++)
                c = (((c & 1) << 7) | ((c & 0xfe) >> 1)) + fn[n];
            u = uc2 + (i - 1) * 13;
            for(n = 0; i--; n += 32, u -= 13) {
                dir[n] = (!n ? 0x40 : 0) | (i + 1);
                dir[n + 0xB] = 0xF;
                dir[n + 0xD] = c;
                memcpy(dir + n + 1, (uint8_t*)u, 10);
                memcpy(dir + n + 14, (uint8_t*)u + 10, 12);
                memcpy(dir + n + 28, (uint8_t*)u + 22, 4);
            }
        }
        if(dir + n + 32 >= ptr + size) {
            /* add new cluster(s) to the parent directory */
            fat_fat32[last] = fat_findclu(((n + 1) * 32 + fat_bpc - 1) / fat_bpc);
        }
        /* add new cluster(s) for file contents */
        if(!isdir && !flen) *clu = 0;
        else *clu = fat_findclu(add == 2 ? (flen + fat_bpc - 1) / fat_bpc : 1);
        fat_dirent83(dir + n, fn, isdir ? 0x10 : 0, *clu, flen);
        n += 32;
        /* write out new directory entry */
        for(s = ptr, i = parent; s + fat_bpc < dir; s += fat_bpc) i = fat_fat32[i];
        if(!dev_write(f, ((data_lba + i * bpb->spc) << 9), s, fat_bpc)) {
            fprintf(stderr, "%ssimpleboot: unable to write '%s'\r\n", verbose ? "\n" : "", out);
        }
        /* long filenames might cross cluster border */
        if(dir + n > s + fat_bpc) {
            i = fat_fat32[i];
            if(!dev_write(f, ((data_lba + i * bpb->spc) << 9), s + fat_bpc, fat_bpc)) {
                fprintf(stderr, "%ssimpleboot: unable to write '%s'\r\n", verbose ? "\n" : "", out);
            }
        }
        /* if we have added a directory, then we must create . and .. entries in it as well */
        if(isdir) {
            memset(fn, ' ', 11); memset(dir, 0, fat_bpc);
            fn[0] = '.'; s = fat_dirent83(dir, fn, 0x10, *clu, 0);
            fn[1] = '.'; s = fat_dirent83(s, fn, 0x10, parent == bpb->rc ? 0 : parent, 0);
            if(!dev_write(f, ((data_lba + *clu * bpb->spc) << 9), dir, fat_bpc)) {
                fprintf(stderr, "%ssimpleboot: unable to write '%s'\r\n", verbose ? "\n" : "", out);
            }
        }
    }
    free(ptr);
    return (!*clu && !flen) || (*clu > 2 && *clu != bpb->rc && *clu < fat_nextcluster);
}

/**
 * Add a file to boot partition
 */
int fat_add(uint32_t parent, char *name, uint8_t *content, uint32_t size)
{
    uint32_t clu, i, nc;

    if(fat_dirent(parent, 1, name, 0, &clu, size) && content && size) {
        /* write out data in cluster sized blocks */
        for(i = 0; i < size; i += fat_bpc) {
            if(!dev_write(f, ((data_lba + clu * bpb->spc) << 9), content, fat_bpc)) {
                fprintf(stderr, "%ssimpleboot: unable to write '%s'\r\n", verbose ? "\n" : "", out);
                return 0;
            }
            if(i + fat_bpc >= size) break;
            content += fat_bpc;
            nc = fat_findclu(1);
            if(!nc) { fat_fat32[clu] = 0xFFFFFFF; return 0; }
            fat_fat32[clu] = nc;
            clu = nc;
        }
        fat_fat32[clu] = 0xFFFFFFF;
        /* if we have remaining data, copy it to an empty cluster and write that */
        if(i < size) {
            memcpy(cluster, content, size - i);
            memset(cluster + size - i, 0, fat_bpc - (size - i));
            if(!dev_write(f, ((data_lba + clu * bpb->spc) << 9), cluster, fat_bpc)) {
                fprintf(stderr, "%ssimpleboot: unable to write '%s'\r\n", verbose ? "\n" : "", out);
                return 0;
            }
        }
    }
    return 1;
}

/**
 * Print status
 */
void status(char *msg, char *par)
{
#ifndef __WIN32__
#define CL "\x1b[K"
#else
#define CL
#endif
    if(verbose) {
        if(!par) printf("\r%-78s" CL "\r", msg);
        else printf("\r%s: %-70s" CL "\r", msg, par);
        fflush(stdout);
    }
}

/**
 * Recursively parse a directory
 */
void parsedir(char *directory, int parent, int calcsize, uint32_t to)
{
#ifdef __WIN32__
    WIN32_FIND_DATAW ffd;
    HANDLE h;
    int i, j;
#else
    DIR *dir;
    struct dirent *ent;
    struct stat st;
    char *s;
#endif
    int64_t dirsize = 0;
    uint32_t clu = to;
    unsigned char *tmp;

    if(!parent) { parent = strlen(directory); skipbytes = parent + 1; strncpy(full, directory, sizeof(full)-1); }
#ifdef __WIN32__
    memset(&szFile, 0, sizeof(szFile));
    MultiByteToWideChar(CP_UTF8, 0, directory, -1, szFile, PATH_MAX);
    for(i = 0; szFile[i]; i++) if(szFile[i] == L'/') szFile[i] = L'\\';
    if(i && szFile[i - 1] != L'\\') szFile[i++] = L'\\';
    wcscpy_s(szFile + i, 255, L"*.*");
    h = FindFirstFileW(szFile, &ffd);
    if(h != INVALID_HANDLE_VALUE) {
        dirsize = 64;
        do {
            if(!wcscmp(ffd.cFileName, L".") || !wcscmp(ffd.cFileName, L"..")) continue;
            wcscpy_s(szFile + i, 255, ffd.cFileName);
            memset(full, 0, sizeof(full)); parent = 0;
            WideCharToMultiByte(CP_UTF8, 0, szFile, -1, full, sizeof(full) - 1, NULL, NULL);
            for(j = 0; full[j]; j++) if(full[j] == '\\') { full[j] = '/'; parent = j; }
            read_size = 0;
            /* no need to check filenames, we've converted it from UCS2 */
            if(calcsize) dirsize += (((wcslen(ffd.cFileName) + 12) / 13) + 1) * 32;
            else status("Adding", full + skipbytes);
            if(ffd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
                if(!calcsize) fat_dirent(to, 1, full + parent + 1, 1, &clu, 0);
                parsedir(full, strlen(full), calcsize, clu);
            } else {
                tmp = readfileall(full, calcsize);
                if(calcsize) totsize += (read_size + 511) & ~511;
                else fat_add(to, full + parent + 1, tmp, read_size);
                if(tmp) free(tmp);
            }
        } while(FindNextFileW(h, &ffd) != 0);
        FindClose(h);
    }
#else
    if((dir = opendir(directory)) != NULL) {
        dirsize = 64;
        while((ent = readdir(dir)) != NULL) {
            if(!strcmp(ent->d_name, ".") || !strcmp(ent->d_name, "..")) continue;
            strncpy(full + parent, "/", sizeof(full)-parent-1);
            strncat(full + parent, ent->d_name, sizeof(full)-parent-1);
            if(stat(full, &st)) continue;
            read_size = 0;
            if(calcsize) {
                /* we must check if all filenames can be encoded as UCS2 */
                for(s = ent->d_name; *s; s++)
                    if((*s & 128) != 0) {
                        if((*s & 32) == 0) s++; else
                        if((*s & 16) == 0) s += 2;
                        else {
                            fprintf(stderr, "%ssimpleboot: unable to encode file name '%s'\r\n", verbose ? "\n" : "", full);
                            exit(1);
                        }
                    }
                dirsize += (((s - ent->d_name + 12) / 13) + 1) * 32;
            } else status("Adding", full + skipbytes);
            if(S_ISDIR(st.st_mode)) {
                if(!calcsize) fat_dirent(to, 1, full + parent + 1, 1, &clu, 0);
                parsedir(full, strlen(full), calcsize, clu);
            } else
            if(S_ISREG(st.st_mode)) {
                tmp = readfileall(full, calcsize);
                if(calcsize) totsize += (read_size + 511) & ~511;
                else fat_add(to, full + parent + 1, tmp, read_size);
                if(tmp) free(tmp);
            }
        }
        closedir(dir);
    }
#endif
    totsize += (dirsize + 511) & ~511;
    if(parent + 1 == skipbytes) skipbytes = 0;
}

/**
 * Open a device
 */
void *dev_open(char *file)
{
#ifdef __WIN32__
    HANDLE f = 0;
    int i;

    memset(&szFile, 0, sizeof(szFile));
    MultiByteToWideChar(CP_UTF8, 0, file, -1, szFile, PATH_MAX);
    for(i = 0; szFile[i]; i++) if(szFile[i] == L'/') szFile[i] = L'\\';
    f = CreateFileW(szFile, GENERIC_READ|GENERIC_WRITE, FILE_SHARE_READ|FILE_SHARE_READ, NULL, OPEN_EXISTING,
        FILE_FLAG_NO_BUFFERING, NULL);
    if(f == INVALID_HANDLE_VALUE) f = NULL;
#else
    char *full, *mnt, *s;
    intptr_t f;
    int l;

    memset(&dev_st, 0, sizeof(dev_st));

    /* resolve relative paths and symlinks first */
    if(!(full = realpath(file, NULL))) full = file;
    /* check if the file is mounted by any chance */
    mnt = (char*)readfileall(
#ifdef __linux__
        "/proc/self/mountinfo"
#else
        "/etc/mtab"
#endif
        , 0);
    if(mnt) {
        for(s = mnt, l = strlen(full); *s; s++) {
            /* find beginning of a line */
            while(*s && (*s == '\r' || *s == '\n' || *s == ' ' || *s == '\t')) s++;
            if(!memcmp(s, full, l)) {
                fprintf(stderr, "simpleboot: device '%s' is mounted. Please umount first.\r\n", file);
                free(mnt); if(full != file) free(full);
                return 0;
            }
            /* skip to the end of the line */
            while(*s && *s != '\r' && *s != '\n') s++;
        }
        free(mnt);
    }
    /* not fopen because this might be an actual device file, in which case we need exclusive access and non-buffered IO */
    stat(full, &dev_st);
    f = open(full, O_RDWR | (S_ISBLK(dev_st.st_mode) ? O_SYNC | O_EXCL : 0));
    if(full != file) free(full);
    if(f < 3) f = 0;
#endif
    return (void *)f;
}

/**
 * Read from the device
 */
int dev_read(void *f, uint64_t off, void *buf, uint32_t size)
{
#ifdef __WIN32__
    DWORD r = size, t;
    LARGE_INTEGER pos;
    pos.QuadPart = off;
    SetFilePointerEx((HANDLE)f, pos, NULL, FILE_BEGIN);
    if(!ReadFile((HANDLE)f, buf, r, &t, NULL) || r != t) return 0;
#else
    if(lseek((intptr_t)f, (off_t)off, SEEK_SET) != (off_t)off || read((intptr_t)f, buf, (ssize_t)size) != (ssize_t)size) return 0;
#endif
    return 1;
}

/**
 * Write to the device
 */
int dev_write(void *f, uint64_t off, void *buf, uint32_t size)
{
#ifdef __WIN32__
    uint8_t sec[512];
    DWORD r = size & ~511, t;
    LARGE_INTEGER pos;
    pos.QuadPart = off;
#endif
    if(verbose > 2) printf("simpleboot: dev_write offset %08lx %d bytes\n", (long unsigned int)off, size);
#ifdef __WIN32__
    SetFilePointerEx((HANDLE)f, pos, NULL, FILE_BEGIN);
    if(!WriteFile((HANDLE)f, buf, r, &t, NULL) || r != t) return 0;
    if(r < size) {
        /* easyboot issue #7: Win reports an 0x57 ERROR_INVALID_PARAMETER if size isn't multiple of 512 */
        memset(sec, 0, 512); memcpy(sec, buf + r, size - r); r = 512;
        if(!WriteFile((HANDLE)f, sec, r, &t, NULL) || r != t) return 0;
    }
#else
    if(lseek((intptr_t)f, (off_t)off, SEEK_SET) != (off_t)off || write((intptr_t)f, buf, (ssize_t)size) != (ssize_t)size) return 0;
#endif
    return 1;
}

/**
 * Close a device
 */
void dev_close(void *f)
{
#ifdef __WIN32__
    CloseHandle((HANDLE)f);
#else
    if(S_ISBLK(dev_st.st_mode)) fdatasync((intptr_t)f);
    close((intptr_t)f);
#endif
}

/**
 * Check the configuration file for validity and correctness
 */
void check_config(char *in)
{
    uint8_t *tmp;
    int line = 1, i, w, h, b, bkp;
    char *buf, *end, *s, *e, *a, *n, *d, fn[sizeof(full)];

    skipbytes = strlen(in) + 1;
    strncpy(full, in, sizeof(full)-1);
    strncat(full, "/simpleboot.cfg", sizeof(full)-1);
    buf = (char*)readfileall(full, 0);
    if(!buf) return;

    for(s = buf, end = buf + read_size; s < end && *s; s = e) {
        /* find beginning of a line */
        while(s < end && *s && (*s == '\r' || *s == '\n' || *s == ' ' || *s == '\t')) { if(*s == '\n') { line++; } s++; }
        for(a = s; a < end && *a && *a != ' ' && *a != '\r' && *a != '\n'; a++);
        for(e = a; e < end && *e && *e != '\r' && *e != '\n'; e++);
        while(a < e && *a == ' ') a++;
        for(n = a; n < e && *n && *n != ' ' && *n != '\r' && *n != '\n'; n++)
            if(*n == '\\' && n[1] == ' ') n++;
        /* 's' points to the start of the command,
         * 'a' to the first argument,
         * 'n' to the end of the first argument (next argument almost), and
         * 'e' to the end of the line */
        if(!memcmp(s, "backup", 6)) { bkp = 1; s += 6; } else bkp = 0;
        if(!memcmp(s, "multicore", 9)) { if(bkp) bkpsmp++; else smp++; } else
        if(!memcmp(s, "verbose", 7)) {
            i = atoi(a);
            if(verbose > 1 && (i < 0 || i > 3)) {
                fprintf(stderr, "%ssimpleboot: bad verbosity level '%s' line %u\r\n", verbose ? "\n" : "", full, line);
                exit(1);
            }
        } else
        if(!memcmp(s, "framebuffer", 11)) {
            if(bkp) bkpfb = 1;
            if(verbose > 1) {
                w = atoi(a); a = n;
                while(a < e && *a == ' ') a++;
                h = atoi(a);
                for(; a < e && *a && *a != ' ' && *a != '\r' && *a != '\n'; a++);
                while(a < e && *a == ' ') a++;
                b = atoi(a);
                if(w < 320 || w > 65536 || h < 200 || h > 65536 || b < 15 || b > 32) {
                    fprintf(stderr, "%ssimpleboot: bad framebuffer line in '%s' line %u\r\n", verbose ? "\n" : "", full, line);
                    exit(1);
                }
                /* only store the config if the command isn't backup prefixed */
                if(!bkp) { fb_w = w; fb_h = h; fb_bpp = b; }
            }
        } else
        if(!memcmp(s, "bootsplash", 10)) {
            if(bkp) bkplogo = 1;
            if(verbose > 1) {
                if(*a == '#') {
                    for(a++, i = 0; i < 6; i++)
                        if(!((a[i] >= '0' && a[i] <= '9') || (a[i] >= 'a' && a[i] <= 'f') || (a[i] >= 'A' && a[i] <= 'F'))) {
                            *n = 0;
                            fprintf(stderr, "%ssimpleboot: not a valid hex color '%s' in '%s' line %u\r\n", verbose ? "\n" : "", a,
                                full, line);
                            exit(1);
                        }
                    while(n < e && *n == ' ') n++;
                    a = n;
                }
                if(a < e) {
                    memcpy(fn, full, skipbytes); memcpy(fn + skipbytes, a, e - a); fn[skipbytes + (uintptr_t)(e - a)] = 0;
                    if(!(tmp = readfileall(fn, 0))) {
                        fprintf(stderr, "%ssimpleboot: unable to load logo '%s' in '%s' line %u\r\n", verbose ? "\n" : "",
                            fn, full, line);
                        exit(1);
                    }
                    if(!tmp || tmp[0] || tmp[1] != 1 || tmp[2] != 9 || tmp[3] || tmp[4] || (tmp[7] != 24 && tmp[7] != 32)) {
                        free(tmp);
                        fprintf(stderr,
                            "%ssimpleboot: '%s' is not an indexed RLE compressed TGA image in '%s' line %u\r\n",
                            verbose ? "\n" : "", fn, full, line);
                        exit(1);
                    }
                    free(tmp);
                }
            }
        } else
        if(!memcmp(s, "kernel", 6)) {
            if(!bkp) {
                if(kernel < (char*)loader_x86_efi || kernel >= (char*)loader_x86_efi + sizeof(loader_x86_efi)) {
                    fprintf(stderr, "%ssimpleboot: kernel already defined in '%s' line %u\r\n", verbose ? "\n" : "", full, line);
                    exit(1);
                }
                kernel = (char*)malloc(n - a + 1); kernelfree = 1;
                if(!kernel) { fprintf(stderr, "%ssimpleboot: unable to allocate memory\r\n", verbose ? "\n" : ""); exit(1); }
                for(d = kernel, s = a; s < n; s++) {
                    if(*s == '\\' && s[1] == ' ') s++;
                    *d++ = *s;
                }
                *d = 0;
            } else
            if(verbose > 1) {
                bkpkrnl = 1;
                memcpy(fn, full, skipbytes); memcpy(fn + skipbytes, a, n - a); fn[skipbytes + (uintptr_t)(n - a)] = 0;
                (void)readfileall(fn, 1);
                if(!read_size) {
                    fprintf(stderr, "%ssimpleboot: unable to load backup kernel '%s' in '%s' line %u\r\n", verbose ? "\n" : "",
                        fn, full, line);
                    exit(1);
                }
            }
        } else
        if(!memcmp(s, "module", 6)) {
            if(verbose > 1) {
                memcpy(fn, full, skipbytes); memcpy(fn + skipbytes, a, n - a); fn[skipbytes + (uintptr_t)(n - a)] = 0;
                (void)readfileall(fn, 1);
                if(!read_size) {
                    fprintf(stderr, "%ssimpleboot: unable to load module '%s' in '%s' line %u\r\n", verbose ? "\n" : "",
                        fn, full, line);
                    exit(1);
                }
            }
            if(bkp) num_bkp++; else num_mod++;
        } else
        if(*s && *s != '#' && *s != '\r' && *s != '\n') {
            if(verbose > 1) {
                while(a > s && a[-1] == ' ') { a--; } *a = 0;
                fprintf(stderr, "%ssimpleboot: unknown command '%s' in '%s' line %u\r\n", verbose ? "\n" : "", s, full, line);
                exit(1);
            }
        }
    }
    free(buf);
    skipbytes = 0;
}

/**
 * Detect the kernel
 */
uint8_t *detect_kernel(char *in, uint8_t **exe, int *arch)
{
    uint8_t *ptr, *ptr2;
    linux_boot_t *hdr;

    if(!in || !*in || !kernel || !*kernel) return NULL;
    *exe = NULL; *arch = 0;
    strncpy(full, in, sizeof(full)-1);
    strncat(full, "/", sizeof(full)-1);
    strncat(full, kernel, sizeof(full)-1);
    ptr = readfileall(full, 0);
    if(ptr) {
        hdr = (linux_boot_t*)(ptr + 0x1f1);
        if(hdr->boot_flag == 0xAA55 && !memcmp(&hdr->header, HDRSMAG, 4)) { *exe = ptr + 0x1f1; return ptr; } else
        if(((mz_hdr*)ptr)->magic == MZ_MAGIC && ((pe_hdr*)(ptr + ((mz_hdr*)ptr)->peaddr))->magic == PE_MAGIC) {
            *exe = ptr + ((mz_hdr*)ptr)->peaddr;
            *arch = ((pe_hdr*)*exe)->machine == IMAGE_FILE_MACHINE_ARM64 ? 1 : 0;
            return ptr;
        } else
        if(!memcmp(((Elf32_Ehdr*)ptr)->e_ident, ELFMAG, 4)) {
            *exe = ptr;
            *arch = ((Elf32_Ehdr*)*exe)->e_machine == EM_AARCH64 ? 1 : 0;
            return ptr;
        } else {
            for(ptr2 = ptr; ptr2 < ptr + read_size - sizeof(Elf32_Ehdr); ptr2 += 16)
                if(!memcmp(((Elf32_Ehdr*)ptr2)->e_ident, ELFMAG, 4)) {
                    *exe = ptr2;
                    *arch = ((Elf32_Ehdr*)*exe)->e_machine == EM_AARCH64 ? 1 : 0;
                    return ptr;
                }
        }
        free(ptr); ptr = NULL;
    }
    return ptr;
}

/**
 * Convert hex string into integer
 */
uint64_t gethex(char *ptr, int len)
{
    uint64_t ret = 0;
    for(;len--;ptr++) {
        if(*ptr>='0' && *ptr<='9') {          ret <<= 4; ret += (unsigned int)(*ptr-'0'); }
        else if(*ptr >= 'a' && *ptr <= 'f') { ret <<= 4; ret += (unsigned int)(*ptr-'a'+10); }
        else if(*ptr >= 'A' && *ptr <= 'F') { ret <<= 4; ret += (unsigned int)(*ptr-'A'+10); }
        else break;
    }
    return ret;
}

/**
 * Parse a GUID in string into binary representation
 */
void getguid(char *ptr, guid_t *guid)
{
    int i;

    if(!ptr || !*ptr || ptr[8] != '-' || ptr[13] != '-' || ptr[18] != '-') return;
    memset(guid, 0, sizeof(guid_t));
    guid->Data1 = gethex(ptr, 8); ptr += 9;
    guid->Data2 = gethex(ptr, 4); ptr += 5;
    guid->Data3 = gethex(ptr, 4); ptr += 5;
    guid->Data4[0] = gethex(ptr, 2); ptr += 2;
    guid->Data4[1] = gethex(ptr, 2); ptr += 2; if(*ptr == '-') ptr++;
    for(i = 2; i < 8; i++, ptr += 2) guid->Data4[i] = gethex(ptr, 2);
}

/**
 * Print usage
 */
void usage(char *cmd)
{
    char *tmp = strrchr(cmd,
#ifdef __WIN32__
    '\\'
#else
    '/'
#endif
    );
    if(!tmp) tmp = cmd; else tmp++;

    printf("Simpleboot installer v%s, Copyright (c) 2023 bzt, MIT license\r\nhttps://codeberg.org/bzt/simpleboot\r\n\r\n", sbver);
    printf("%s [-v|-vv] [-k <name>] [-i <name>] [-m] [-s <mb>] [-b <mb>]\r\n", tmp);
    printf("   [-u <guid>] [-p <t> <u> <i>] [-r|-e] [-c] <indir> <outfile|device>\r\n\r\n");
    printf("  -v, -vv         increase verbosity / validation\r\n");
    printf("  -k <name>       set the default kernel filename (defaults to 'kernel')\r\n");
    printf("  -i <name>       set the default initrd filename (by default none)\r\n");
    printf("  -m              set multicore to enabled (by default disabled)\r\n");
    printf("  -s <mb>         set the disk image size in Megabytes (defaults to 35M)\r\n");
    printf("  -b <mb>         set the boot partition size in Megabytes (defaults to 33M)\r\n");
    printf("  -u <guid>       set the boot partition unique identifier (defaults to random)\r\n");
    printf("  -p <t> <u> <i>  add an extra partition (type guid, unique guid, imagefile)\r\n");
    printf("  -r              place loaders in ROM (by default save them into the image)\r\n");
    printf("  -e              add El Torito Boot Catalog (BIOS / EFI CDROM boot support)\r\n");
    printf("  -c              always create a new image file even if it exists\r\n");
    printf("  indir           use the contents of this directory for the boot partition\r\n");
    printf("  outfile         output image file or device name\r\n");
    printf("\r\n Flags -k, -i, -m only needed if you don't have a simpleboot.cfg file.\r\n");
    printf(" Loader versions: %08x (x86), %08x (rpi)\r\n\r\n", ts_x86, ts_rpi);
    exit(1);
}

/**
 * Main function
 */
int not_main(int argc, char **argv)
{
    time_t t;
    int64_t siz;
    uint32_t clu, loader_lba = 0;
    int i, j, arch = 0, docalc = 1, eltorito = 0, rom = 0, create = 0, smpwarn = 0, defsmp = 0;
    char *defkernel = NULL, *definitrd = NULL;
    guid_t espGuid = EFI_PART_TYPE_EFI_SYSTEM_PART_GUID;
    uint8_t *ptr = loader_x86_efi + ((mz_hdr*)loader_x86_efi)->peaddr, *ph, *exe = NULL;
    pe_sec *sec = (pe_sec*)(ptr + ((pe_hdr*)ptr)->opt_hdr_size + 24);
    fb_boot_t *fbcat;
    uint32_t o, sectop = 0;
    linux_boot_t *hdr;

    /* get random GUIDs */
    t = time(NULL); fat_ts = gmtime(&t); srand(t);
    i = rand(); memcpy(&((uint8_t*)&dguid)[0], &i, 4); i = rand(); memcpy(&((uint8_t*)&dguid)[4], &i, 4);
    i = rand(); memcpy(&((uint8_t*)&dguid)[8], &i, 4); i = rand(); memcpy(&((uint8_t*)&dguid)[12], &i, 4);
    i = rand(); memcpy(&((uint8_t*)&pguid)[0], &i, 4); i = rand(); memcpy(&((uint8_t*)&pguid)[4], &i, 4);
    i = rand(); memcpy(&((uint8_t*)&pguid)[8], &i, 4); i = rand(); memcpy(&((uint8_t*)&pguid)[12], &i, 4);

    /* get loader versions */
    ts_x86 = ((pe_hdr*)ptr)->timestamp;
    ts_rpi = crc32_calc(loader_rpi_bin, sizeof(loader_rpi_bin));

    /* parse command line */
    for(i = 1; i < argc && argv[i]; i++)
        if(argv[i][0] == '-') {
            switch(argv[i][1]) {
                case 'k': defkernel = argv[++i]; break;
                case 'i': definitrd = argv[++i]; break;
                case 's': disksize = atoi(argv[++i]); if(docalc) { partsize = disksize - 2; } docalc = 0; break;
                case 'b': partsize = atoi(argv[++i]); docalc = 0; break;
                case 'u': getguid(argv[++i], &pguid); break;
                case 'p':
                    if(i + 3 >= argc || nump + 1 >= (int)(sizeof(ptguid)/sizeof(ptguid[0]))) usage(argv[0]);
                    getguid(argv[++i], &ptguid[nump]); getguid(argv[++i], &puguid[nump]); pfile[nump++] = argv[i];
                break;
                default:
                    for(j = 1; argv[i][j]; j++)
                        switch(argv[i][j]) {
                            case 'v': verbose++; break;
                            case 'm': defsmp++; break;
                            case 'r': rom = 1; break;
                            case 'e': eltorito = 1; break;
                            case 'c': create = 1; break;
                            default: usage(argv[0]); break;
                        }
                break;
            }
        } else
        if(!in) in = argv[i]; else
        if(!out) out = argv[i]; else
            usage(argv[0]);
    if(!out) usage(argv[0]);

    /* self-integrity check */
    for(i = 0, sectop = 0; i < (int)sizeof(loader_rpi_bin) - 128; i += 16)
        if(!memcmp(loader_rpi_bin + i, "kernel\0\0\0\0\0\0\0\0\0", 16)) {
            /* patch default filenames in the loader */
            if(defkernel && *defkernel) strncpy((char*)loader_rpi_bin + i, defkernel, 63);
            if(definitrd && *definitrd) strncpy((char*)loader_rpi_bin + i + 64, definitrd, 62);
            if(defsmp) loader_rpi_bin[i + 127] = 1;
            sectop = *((uint32_t*)(loader_rpi_bin + i + 128 + 4));
            if(verbose > 2) printf("loader_rpi _bss_start %08x _bss_end %08x\r\n", *((uint32_t*)(loader_rpi_bin + i + 128)),
                sectop);
            break;
        }
    if(i >= (int)sizeof(loader_rpi_bin) - 128 || !sectop || sectop >= 0xA0000) {
        fprintf(stderr, "simpleboot: invalid inlined loader_rpi???\r\n");
        return 1;
    }
    for(i = 0, sectop = 0; i < ((pe_hdr*)ptr)->sections; i++, sec++) {
        if(!strcmp(sec->name, ".rdata")) {
            for(kernel = (char*)loader_x86_efi + sec->raddr; strcmp(kernel, "kernel"); kernel++);
            /* patch default filenames in the loader */
            if(defkernel && *defkernel) strncpy(kernel, defkernel, 63);
            if(definitrd && *definitrd) strncpy(kernel + 64, definitrd, 62);
            if(defsmp) kernel[127] = 1;
        }
        o = sec->vaddr + 0x8000 - ((pe_hdr*)ptr)->code_base + sec->vsiz;
        if(o > sectop) sectop = o;
        if(verbose > 2) printf("loader_x86 section '%-8s' %08x %5u rel %08x top %08x\r\n", sec->name, sec->vaddr, sec->vsiz,
            o - sec->vsiz, sectop);
    }
    /* max size comes from boot_x86.asm line 72 times two, sectop is the same plus the bss size */
    if(sizeof(loader_x86_efi) > 120 * 2 * 512 || !sectop || sectop > 0x20000 || !kernel) {
        fprintf(stderr, "simpleboot: invalid inlined loader_x86???\r\n");
        return 1;
    }

    /* calculate the required minimum partition size */
    totsize = 0;
    parsedir(in, 0, 1, 0);
    skipbytes = 0;
    if(totsize < 1) {
        fprintf(stderr, "simpleboot: input directory not found '%s'\r\n", in);
        return 1;
    }
    /* add FAT table size and round up to Megabytes */
    totsize += (8 + (((totsize / 512) * 4) / 512) * 2) * 512;
    totsize = (totsize + 1024 * 1024 - 1) & ~(1024 * 1024 - 1);
    if(totsize / 1024 > 2047 * 1024) {
        fprintf(stderr, "simpleboot: more than 2 Gb contents in '%s'\r\n", in);
        return 1;
    }

    /* check configuration and kernel */
    i = strlen(in);
    if(in[i - 1] == '/') in[i - 1] = 0;
    check_config(in);
    if(!(ptr = detect_kernel(in, &exe, &arch))) { fprintf(stderr, "simpleboot: kernel file '%s' not found\r\n", full); return 1; }
    if(rom && arch) { fprintf(stderr, "simpleboot: legacy ROM booting only supported on x86\r\n"); return 1; }
    if(rom && eltorito) { fprintf(stderr, "simpleboot: El Torito and legacy ROM booting are mutually exclusive\r\n"); return 1; }

    /* create image if it doesn't exists */
    if(create || !(f = dev_open(out))) {
        /* check partition and disk sizes */
        if(partsize == 2048) partsize = 2047;
        if(partsize < 33 || partsize > 2047) {
            fprintf(stderr, "simpleboot: bad partition size, must be between 33 Mb and 2 Gb\r\n");
            return 1;
        }
        disksize *= 2048 * 512;
        partsize *= 2048 * 512;
        /* expand required minimum partition size (only if neither "-p" nor "-d" given) */
        if(docalc && totsize > partsize) {
            partsize = totsize;
            if(verbose) printf("simpleboot: expanding partition size to %lu Mb\r\n",
                (unsigned long int)(partsize / 1024L / 1024L));
        }
        siz = partsize + (FIRST_PARTITION + GPT_SECTORS + 1) * 512;
        for(i = 0; i < nump; i++) {
            if(pfile[i]) { readfileall(pfile[i], 1); psize[i] = read_size; }
            siz += (psize[i] + 1024*1024-1) & ~(1024*1024-1);
        }
        if(disksize < siz) {
            disksize = siz;
            if(verbose) printf("simpleboot: expanding disk size to %lu Mb\r\n",
                (unsigned long int)(disksize / 1024 / 1024));
        }
#ifdef __WIN32__
        MultiByteToWideChar(CP_UTF8, 0, out, -1, szFile, PATH_MAX);
        for(i = 0; szFile[i]; i++) if(szFile[i] == L'/') szFile[i] = L'\\';
        f = CreateFileW(szFile, GENERIC_WRITE, FILE_SHARE_WRITE, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
#else
        umask(0111);
        f = (void*)(intptr_t)open(out, O_WRONLY | O_CREAT | O_TRUNC, 0666);
#endif
        if(!f) { fprintf(stderr, "simpleboot: unable to write '%s'\r\n", out); return 2; }

        imgsize = ((FIRST_PARTITION + 2) << 9) + partsize;
        if(!(img = (uint8_t*)malloc(imgsize))) { fprintf(stderr, "simpleboot: unable to allocate memory\r\n"); return 1; }
        /* generate disk image */
        status("Formatting...", NULL);
        memset(img, 0, imgsize); fs_base = img + FIRST_PARTITION * 512;
        gpt_create();
        if(eltorito) etbc_create();
        fat_format();
        if(!dev_write(f, 0, img, imgsize) || !dev_write(f, disksize - sizeof(img_tail), img_tail, sizeof(img_tail))) {
            dev_close(f); free(img); fprintf(stderr, "simpleboot: unable to write '%s'\r\n", out); return 2;
        }
        free(img); img = NULL;
        /* write optional extra partition(s) */
        for(siz = (FIRST_PARTITION * 512 + partsize + 1024*1024-1) & ~(1024*1024-1), i = 0; i < nump; i++)
            if(pfile[i] && (pdata = (uint8_t*)readfileall(pfile[i], 0))) {
                if(!dev_write(f, siz, pdata, psize[i])) {
                    dev_close(f); free(pdata); fprintf(stderr, "simpleboot: unable to write '%s'\r\n", out); return 2;
                }
                free(pdata); pdata = NULL;
                siz += (psize[i] + 1024*1024-1) & ~(1024*1024-1);
            }
        dev_close(f);
        if(!(f = dev_open(out))) { fprintf(stderr, "simpleboot: unable to open '%s'\r\n", out); return 2; }
    }

    if(!(img = (uint8_t*)malloc(65536+1024))) { fprintf(stderr, "simpleboot: unable to allocate memory\r\n"); return 1; }
    fs_base = img + 1024; bpb = (esp_bpb_t*)fs_base;

    /* locate EFI System Partition */
    if(!dev_read(f, 0, img, 2 * 512)) {
        fprintf(stderr, "simpleboot: unable to read '%s'\r\n", out); goto err;
    }
    if(!memcmp(img + 512, EFI_PTAB_HEADER_ID, 8)) {
        /* found GPT */
        if(verbose) printf("\r\nGUID Partitioning Table found at LBA %lu\r\n",
            (long unsigned int)((gpt_header_t*)(img + 512))->PartitionEntryLBA);
        memcpy(&dguid, &((gpt_header_t*)(img + 512))->DiskGUID, sizeof(guid_t));
        j = ((gpt_header_t*)(img + 512))->SizeOfPartitionEntry;
        /* look for ESP in the first 8 sectors only. Should be the very first entry anyway */
        if(!dev_read(f, ((gpt_header_t*)(img + 512))->PartitionEntryLBA << 9, fs_base, 8 * 512)) goto noesp;
        for(i = 0; i + j <= 8 * 512; i += j)
            if(!memcmp(&((gpt_entry_t*)&fs_base[i])->PartitionTypeGUID, &espGuid, sizeof(guid_t))) {
                boot_lba = ((gpt_entry_t*)&fs_base[i])->StartingLBA;
                partsize = (((gpt_entry_t*)&fs_base[i])->EndingLBA - ((gpt_entry_t*)&fs_base[i])->StartingLBA + 1) << 9;
                memcpy(&pguid, &((gpt_entry_t*)&fs_base[i])->UniquePartitionGUID, sizeof(guid_t));
                /* create hybrid MBR Partitioning scheme */
                if(img[0x1C2] != 0xC || *((uint32_t*)(img+0x1C0+6)) != boot_lba || *((uint32_t*)(img+0x1C0+10)) != partsize >> 9) {
                    img[0x1C2] = 0xC;
                    *((uint32_t*)(img+0x1C0+6)) = boot_lba;
                    *((uint32_t*)(img+0x1C0+10)) = partsize >> 9;
                    chs(boot_lba, img+0x1BE);
                    chs(boot_lba+partsize/512-1, img+0x1C2);
                }
                img[0x1BE] = 0x80; img[0x1CE] = img[0x1DE] = img[0x1EE] = 0;
                if(!*((uint32_t*)(img+0x1B8))) memcpy(img+0x1B8, &dguid.Data1, 4);
                break;
            }
    } else
    if(img[510] == 0x55 && img[511] == 0xAA) {
        /* fallback to MBR partitioning scheme */
        if(verbose) printf("\r\nPMBR DOS Partitioning Table found at LBA 0\r\n");
        for(i = 0x1c0; i < 510; i += 16)
            if(img[i - 2] == 0x80/*active*/ && (img[i + 2] == 0xC/*FAT32*/ || img[i + 2] == 0xEF/*ESP*/)) {
                boot_lba = (uint64_t)(*((uint32_t*)(img + i + 6)));
                partsize = (uint64_t)*((uint32_t*)(img + i + 10)) << 9;
                break;
            }
        j = 0;
    }
    if(!boot_lba || !dev_read(f, boot_lba << 9, fs_base, 65536) || bpb->bps != 512 || !bpb->spc || bpb->spf16 || !bpb->spf32 ||
      fs_base[0x32] > 127) {
noesp:  if(verbose) printf("BPB bps %u spc %u spf16 %u spf32 %u BPB copy LBA %u\r\n", bpb->bps, bpb->spc, bpb->spf16, bpb->spf32,
            fs_base[0x32]);
        fprintf(stderr, "simpleboot: unable to locate boot partition in '%s'\r\n", out); goto err;
    }
    if(verbose) {
        printf("Boot partition found at LBA %lu", (long unsigned int)boot_lba);
        if(j) printf(", UUID: %08X-%04X-%04X-%02X%02X%02X%02X%02X%02X%02X%02X\r\n", pguid.Data1, pguid.Data2,
                pguid.Data3, pguid.Data4[0], pguid.Data4[1], pguid.Data4[2], pguid.Data4[3], pguid.Data4[4], pguid.Data4[5],
                pguid.Data4[6], pguid.Data4[7]);
        else printf("\r\n");
        if(verbose > 1)
            printf("BPB bps %u spc %u spf16 %u spf32 %u BPB copy LBA %u\r\n", bpb->bps, bpb->spc, bpb->spf16, bpb->spf32, fs_base[0x32]);
    }
    /* add loaders' size, 2 for the directories and 1 plus per loader because they are not multiple of cluster size */
    if(totsize + 4 * fat_bpc + (int64_t)sizeof(loader_x86_efi) + (int64_t)sizeof(loader_rpi_bin) > partsize) {
        fprintf(stderr, "simpleboot: not enough free space on boot partition\r\n"); goto err;
    }

    if(!(fat_fat32 = (uint32_t*)malloc(bpb->spf32 << 9))) {
        fprintf(stderr, "simpleboot: unable to allocate memory\r\n"); goto err;
    }
    fat_nextcluster = *((uint32_t*)(fs_base + 0x3EC)) + 2; fat_numclu = partsize >> 9;
    fat_freeclu = (int64_t)*((uint32_t*)(fs_base + 0x3E8));
    fat_spf = bpb->spf32; fat_bpc = bpb->spc << 9; data_lba = bpb->spf32 * bpb->nf + bpb->rsc - 2 * bpb->spc + boot_lba;
    if(!(cluster = (uint8_t*)malloc(fat_bpc))) {
        fprintf(stderr, "simpleboot: unable to allocate memory\r\n"); goto err;
    }
    /* load FAT */
    if(!dev_read(f, (bpb->rsc + boot_lba) << 9, fat_fat32, fat_spf << 9)) {
        fprintf(stderr, "simpleboot: unable to read '%s'\r\n", out); goto err;
    }

    /* add files from the given directory */
    parsedir(in, 0, 0, bpb->rc);

    /* add loaders */
    if(!rom) {
        /* add a FOSSBIOS Boot Catalog after the GPT header */
        fbcat = (fb_boot_t*)(img + 512 + 128);
        fbcat->magic[0] = 0xB0; fbcat->magic[1] = 0x07; fbcat->magic[2] = 0xCA; fbcat->magic[3] = 0x7A;
        fbcat->magic[4] = 0x10; fbcat->magic[5] = 0xC0; fbcat->numentries = 2;

        /* stage1 is not a file, it's in the boot sector */
        memcpy(img, boot_x86_bin, 0x1b8); img[0x1FE] = 0x55; img[0x1FF] = 0xAA;
        /* we can't use fat_add() for stage2, because it must be defragmented, stored on contiguous clusters */
        if(fat_dirent(bpb->rc, 1, "EFI", 1, &clu, 0) && fat_dirent(clu, 1, "BOOT", 1, &clu, 0) &&
          fat_dirent(clu, 2, "BOOTX64.EFI", 0, &clu, sizeof(loader_x86_efi)) &&
          dev_write(f, (data_lba + clu * bpb->spc) << 9, loader_x86_efi, sizeof(loader_x86_efi))) {
            fbcat->entries[0].arch = 0;     /* x86 */
            fbcat->entries[0].wordsize = 8; /* 64-bit */
            fbcat->entries[0].lba = *((uint32_t*)(img + 0x1b0)) = loader_lba = data_lba + clu * bpb->spc;
        } else { fprintf(stderr, "simpleboot: unable to write x86 loader '%s'\r\n", out); goto err; }

        /* we can't use fat_add() for stage2, because it must be defragmented, stored on contiguous clusters */
        if(fat_dirent(bpb->rc, 2, "KERNEL8.IMG", 0, &clu, sizeof(loader_rpi_bin)) &&
          dev_write(f, (data_lba + clu * bpb->spc) << 9, loader_rpi_bin, sizeof(loader_rpi_bin))) {
            fbcat->entries[1].arch = 1;     /* ARM */
            fbcat->entries[1].wordsize = 8; /* 64-bit */
            fbcat->entries[1].lba = data_lba + clu * bpb->spc;
        } else { fprintf(stderr, "simpleboot: unable to write RPi loader '%s'\r\n", out); goto err; }

        /* calculate checksum */
        for(i = j = 0; i < 3 * 8; i++) { j += *(((uint8_t*)fbcat) + i); }
        fbcat->chksum = 0x100 - j;
    }

    /* write out metadata */
    fat_finish();
    if(!dev_write(f, boot_lba << 9, bpb, bpb->rsc << 9)) {
        fprintf(stderr, "simpleboot: unable to write BPB to '%s'\r\n", out); goto err;
    }
    for(i = 0; (uint8_t)i < bpb->nf; i++) {
        data_lba = (fat_spf * i + bpb->rsc + boot_lba) << 9; j = fat_spf << 9;
        if(!dev_write(f, data_lba, fat_fat32, j)) {
            fprintf(stderr, "simpleboot: unable to write FAT to '%s'\r\n", out); goto err;
        }
    }
    if(!dev_write(f, 0, img, 1024)) {
        fprintf(stderr, "simpleboot: unable to write PMBR to '%s'\r\n", out); goto err;
    }
    if(verbose) {
        clu = (fat_numclu - fat_freeclu) * 1000 / fat_numclu;
        printf("Clusters total %u last %u free %u %u.%u%%\r\n", (uint32_t)fat_numclu, (uint32_t)fat_nextcluster,
            (uint32_t)fat_freeclu, clu / 10, clu % 10);
    }

    free(img);
    free(fat_fat32);
    free(cluster);
    dev_close(f);
    if(rom) rom_create(out);

    if(verbose) {
        printf("\r%-40s\n", "OK");
        if(verbose > 1) {
            /* print out detailed information */
            printf("\r\nPartition UUID:    %08X-%04X-%04X-%02X%02X%02X%02X%02X%02X%02X%02X\r\n", pguid.Data1, pguid.Data2,
                pguid.Data3, pguid.Data4[0], pguid.Data4[1], pguid.Data4[2], pguid.Data4[3], pguid.Data4[4], pguid.Data4[5],
                pguid.Data4[6], pguid.Data4[7]);
            printf("Boot partition:    start LBA %u, size %u sectors\r\n", FIRST_PARTITION, (uint32_t)(partsize >> 9));
            switch(arch) {
                case 1:
                    printf("Loader (rpi):      \"kernel8.img\" (%08x)\r\n", ts_rpi);
                break;
                default:
                    if(rom)
                        printf("Loader (x86):      in ROM, size %u bytes (%08x)\r\n",
                            (uint32_t)(512 + ((sizeof(loader_x86_efi) + 511) & ~511)), ts_x86);
                    else
                        printf("Loader (x86):      start LBA %u, size %u sectors (%08x)\r\n", loader_lba,
                            (uint32_t)((sizeof(loader_x86_efi) + 511) >> 9), ts_x86);
                break;
            }
            printf("Kernel:            \"%s\"\r\n"
                   "Format:            ", kernel);
            if(exe) {
                hdr = (linux_boot_t*)exe;
                if(hdr->boot_flag == 0xAA55 && !memcmp(&hdr->header, HDRSMAG, 4)) {
                    if(smp) smpwarn = 1;
                    printf("Kernel with Linux/x86 Boot Protocol v%u.%u", hdr->version >> 8, hdr->version & 0xff);
                    if(hdr->version < 0x20c)
                        printf("\r\n                   (invalid, boot protocol version %x too old)", hdr->version);
                    else
                    if((hdr->pref_address + hdr->init_size) >> 32L)
                        printf("\r\n                   (invalid, load address above 4G)");
                    else {
                        printf("\r\n                   %08x - %08x zero page + cmdline"
                               "\r\n                   %08x - %08x kernel image (setup_sects %d, file offs %x)",
                                0x90000, 0x9A000, (uint32_t)hdr->pref_address, (uint32_t)hdr->pref_address + hdr->init_size,
                                hdr->setup_sects, ((hdr->setup_sects ? hdr->setup_sects : 4) + 1) * 512);
                    }
                } else
                if(!memcmp(((Elf32_Ehdr*)exe)->e_ident, ELFMAG, 4)) {
                    /* 32-bit mode only supported with ELF Multiboot2 kernels */
                    j = ((Elf32_Ehdr*)exe)->e_ident[EI_CLASS] == ELFCLASS64 ? 64 : 32;
                    if(smp && j != 64) smpwarn = 1;
                    printf("Multiboot2 ELF%u kernel (%s)", j, j == 32 && ((Elf32_Ehdr*)exe)->e_machine == EM_386 ? "i386" :
                        (j == 64 && ((Elf32_Ehdr*)exe)->e_machine == EM_X86_64 ? "x86_64" :
                        (j == 64 && ((Elf32_Ehdr*)exe)->e_machine == EM_AARCH64 ? "Aarch64" : "invalid architecture")));
                    if(j == 64) {
                        ph = exe + ((Elf64_Ehdr*)exe)->e_phoff;
                        for(i = 0; i < ((Elf64_Ehdr*)exe)->e_phnum; i++, ph += ((Elf64_Ehdr*)exe)->e_phentsize) {
                            if(((Elf64_Phdr*)ph)->p_type == PT_DYNAMIC)
                                printf("\r\n                   (invalid, dynamically linked kernel?)");
                            else
                            if(((Elf64_Phdr*)ph)->p_type == PT_INTERP)
                                printf("\r\n                   (invalid, not freestanding kernel?)");
                            else
                            if(((Elf64_Phdr*)ph)->p_type == PT_LOAD) {
                                printf("\r\n                   %08lx - %08lx %c%c%c",
                                    (long unsigned int)(((Elf64_Phdr*)ph)->p_vaddr),
                                    (long unsigned int)(((Elf64_Phdr*)ph)->p_vaddr + ((Elf64_Phdr*)ph)->p_memsz),
                                    ((Elf64_Phdr*)ph)->p_flags & PF_R ? 'r' : '.', ((Elf64_Phdr*)ph)->p_flags & PF_W ? 'w' : '.',
                                    ((Elf64_Phdr*)ph)->p_flags & PF_X ? 'x' : '.');
                            }
                            if(ph + ((Elf64_Ehdr*)exe)->e_phentsize > exe + 4096)
                                printf(" (invalid, segment descriptor outside of the first page)");
                        }
                    } else {
                        ph = exe + ((Elf32_Ehdr*)exe)->e_phoff;
                        for(i = 0; i < ((Elf32_Ehdr*)exe)->e_phnum; i++, ph += ((Elf32_Ehdr*)exe)->e_phentsize) {
                            if(((Elf32_Phdr*)ph)->p_type == PT_DYNAMIC)
                                printf("\r\n                   (invalid, dynamically linked kernel?)");
                            else
                            if(((Elf32_Phdr*)ph)->p_type == PT_INTERP)
                                printf("\r\n                   (invalid, not freestanding kernel?)");
                            else
                            if(((Elf32_Phdr*)ph)->p_type == PT_LOAD)
                                printf("\r\n                   %08x - %08x rwx", ((Elf32_Phdr*)ph)->p_vaddr,
                                    ((Elf32_Phdr*)ph)->p_vaddr + ((Elf32_Phdr*)ph)->p_memsz);
                            if(ph + ((Elf32_Ehdr*)exe)->e_phentsize > exe + 4096)
                                printf(" (invalid, segment descriptor outside of the first page)");
                        }
                    }
                } else
                if(((pe_hdr*)exe)->magic == PE_MAGIC) {
                    sec = (pe_sec*)(exe + ((pe_hdr*)exe)->opt_hdr_size + 24);
                    j = ((pe_hdr*)exe)->file_type == PE_OPT_MAGIC_PE32PLUS ? 64 : 32;
                    o = j == 64 ? (uint32_t)((pe_hdr*)exe)->data.pe64.img_base : (uint32_t)((pe_hdr*)exe)->data.pe32.img_base;
                    if(smp && j != 64) smpwarn = 1;
                    printf("Multiboot2 PE%u kernel (%s)", j,
                        ((pe_hdr*)exe)->machine == IMAGE_FILE_MACHINE_I386 ? "i386" : (
                        ((pe_hdr*)exe)->machine == IMAGE_FILE_MACHINE_AMD64 ? "x86_64" : (
                        ((pe_hdr*)exe)->machine == IMAGE_FILE_MACHINE_ARM64 ? "Aarch64" : "invalid architecture")));
                    for(i = 0; i < ((pe_hdr*)exe)->sections; i++, sec++) {
                        printf("\r\n                   %08x - %08x %s", o + sec->vaddr, o + sec->vaddr + sec->vsiz, sec->name);
                        if((uint8_t*)&sec[1] > exe + 4096)
                            printf(" (invalid, segment descriptor outside of the first page)");
                    }
                } else printf("(unknown kernel format?)");
            } else printf("(file not found?)");
            if(!num_mod && definitrd && *definitrd) {
                strncpy(full + skipbytes, definitrd, sizeof(full) - 1 - skipbytes);
                (void)readfileall(full, 1);
                if(read_size) num_mod++;
            }
            printf("\r\nSMP multicore:     %s\r\n", smpwarn ? "disabled (only supported with MB64)" : (smp ? "yes" : "no"));
            printf("Number of modules: %u file%s\r\n", num_mod, num_mod > 1 ? "s" : "");
            printf("Framebuffer:       %u x %u pixels, %u bits per pixel\r\n", fb_w, fb_h, fb_bpp);
            printf("Emergency backup: %s%s%s%s%s", bkpkrnl | bkplogo | bkpfb || num_bkp > 0 ? "" : " (not configured)",
                bkpkrnl ? " kernel" : "", bkpfb ? " framebuffer" : "", bkplogo ? " bootsplash" : "", bkpsmp ? " multicore" : "");
            if(num_bkp > 0) printf(" module(%u)", num_bkp);
            printf("\r\nCDROM bootable:    %s\r\n\r\n", eltorito ? "yes" : "no");
        }
    } else { verbose = 1; status("OK\n", NULL); }
    if(kernelfree) free(kernel);
    return 0;
err:
    if(img) free(img);
    if(fat_fat32) free(fat_fat32);
    if(cluster) free(cluster);
    dev_close(f);
    return 1;
}
