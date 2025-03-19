;
;  src/rombios_x86.asm
;  https://codeberg.org/bzt/simpleboot
;
;  Copyright (C) 2023 bzt, MIT license
;
;  Permission is hereby granted, free of charge, to any person obtaining a copy
;  of this software and associated documentation files (the "Software"), to
;  deal in the Software without restriction, including without limitation the
;  rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
;  sell copies of the Software, and to permit persons to whom the Software is
;  furnished to do so, subject to the following conditions:
;
;  The above copyright notice and this permission notice shall be included in
;  all copies or substantial portions of the Software.
;
;  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
;  IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
;  FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.  IN NO EVENT SHALL ANY
;  DEVELOPER OR DISTRIBUTOR BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
;  WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
;  IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
;
;  @brief Legacy mode BIOS Option ROM for Simpleboot, written with
;      flatassembler: https://flatassembler.net It's job is to extract loader_x86.efi's
;      sections from the ROM buffer, set up long mode and pass control to it
;
;  Memory layout on handover:
;        0h -   400h    IVT (must be preserved)
;      400h -   4FFh    BDA (must be preserved)
;      4FFh -   500h    BIOS boot drive code
;      500h -   510h    BIOS LBA packet
;      510h -   550h    GDT
;      550h -  1000h    stack (2k+)
;     1000h -  8000h    paging tables
;     8000h - 60000h    loader_x86.efi's sections
;    9A000h - A0000h    EBDA (must be preserved)

simpleboot_addr equ     08000h

            ORG         07C00h
            USE16

;*********************************************************************
;*                             code                                  *
;*********************************************************************
rom_header: db          55h, 0AAh
.size:      db          0
            jmp         short @f
            nop
.checksum:  db          0
            ;---- set up environment ----
@@:         cli
            cld
            mov         al, 0FFh                ; disable PIC
            out         021h, al
            out         0A1h, al
            in          al, 70h                 ; disable NMI
            or          al, 80h
            out         70h, al
            xor         ax, ax
            mov         es, ax
            mov         ss, ax
            mov         sp, 07C00h
            ; find our position in memory.
            push        cs
            pop         ds
            xor         esi, esi
            call        @f
@@:         pop         si
            sub         si, @b-rom_header
            xor         eax, eax
            xor         ebp, ebp                ; ebp = ROM payload
            mov         bp, si
            add         bp, rom_end-rom_header
            mov         ax, cs
            shl         eax, 4
            add         ebp, eax
            mov         di, sp
            ;---- relocate ourselves to 0:7C00 ----
            mov         cx, 100h
            repnz       movsw
            jmp         0:@f
@@:         xor         ax, ax
            mov         ds, ax
            mov         byte [4FFh], 80h        ; BIOS drive code
            ;---- enable protmode ----
            mov         ax, 2401h               ; enable A20
            int         15h
            mov         si, GDT_value
            mov         di, 510h
            mov         cx, word[si]
            repnz       movsb
            lgdt        [510h]
            mov         eax, cr0
            or          al, 1
            mov         cr0, eax
            jmp         16:@f
            USE32
@@:         mov         ax, 24
            mov         ds, ax
            mov         es, ax
            ; look for long mode supported flag
            xor         edx, edx
            mov         eax, 80000001h
            cpuid
            bt          edx, 29
            jnc         .die
            ;---- enable longmode ----
            xor         eax, eax
            mov         ah, 010h
            mov         cr3, eax
            ; we only map 2M here, loader will finish up the rest overwriting us in the process
            mov         edx, eax                ; PML4
            mov         ebx, eax
            xor         eax, eax
            mov         dword [ebx], 02003h     ; pointer to 2M PDPE
            mov         dword [ebx + 4], eax
            add         ebx, edx                ; 2M PDPE
            mov         dword [ebx], 03003h
            mov         dword [ebx + 4], eax
            add         ebx, edx                ; 2M PDE
            mov         dword [ebx], 00083h
            mov         dword [ebx + 4], eax
            mov         al, 0E0h                ; set PAE, MCE, PGE; clear everything else
            mov         cr4, eax
            mov         ecx, 0C0000080h         ; EFER MSR
            rdmsr
            bts         eax, 8                  ; enable long mode page tables
            wrmsr
            mov         eax, cr0
            xor         cl, cl
            or          eax, ecx
            btc         eax, 16                 ; clear WP
            mov         cr0, eax                ; enable paging with cache disabled (set PE, CD)
            lgdt        [510h]                  ; read 80 bit address (16+64)
            jmp         32:@f
            USE64
@@:         xor         rax, rax                ; load long mode segments
            mov         ax, 40
            mov         ds, ax
            mov         es, ax
            mov         ss, ax
            mov         ax, simpleboot_addr
            ;---- parse loader_x86.efi (PE / COFF format) ----
            mov         ebx, ebp                ; load buffer address
            cmp         word [ebx], 5A4Dh       ; check MZ
            jne         .die
            mov         r8d, ebx
            xor         r9, r9
            add         ebx, dword [ebx + 0x3c] ; get COFF header
            cmp         word [ebx], 4550h       ; check PE
            jne         .die
            mov         dl, byte [ebx + 6]      ; number of sections
            mov         r9d, dword [ebx + 0x28] ; entry point
            mov         ebp, dword [ebx + 0x2c] ; code base
            add         r9d, eax
            sub         r9d, ebp
            add         bx, word [ebx + 0x14]   ; add header size
            add         bx, 24                  ; ebx now points to section table
@@:         mov         edi, dword [ebx + 12]   ; copy sections from PE into VA
            add         edi, eax
            sub         edi, ebp                ; dest: virtual address + reloc offset - code base
            mov         ecx, dword [ebx + 16]   ; size of raw data
            mov         esi, dword [ebx + 20]
            add         esi, r8d                ; source: pointer to raw data + load offset
            repnz       movsb
            add         ebx, 40                 ; go to next section
            dec         dl
            jnz         @b
            xor         rsp, rsp
            mov         sp, 1000h               ; set stack
            xor         rcx, rcx                ; image handle
            xor         rdx, rdx                ; system table pointer
            jmp         r9                      ; jump to relocated entry point
            ;---- die function ----
            ; written in a way that it's decodeable as prot mode as well as long mode instructions
.die:       mov         esi, .err
            mov         edi, 0B8000h
            mov         ah, 04fh
@@:         lodsb
            or          al, al
            jz          @f
            stosw
            jmp         @b
@@:         hlt
            align       8

;*********************************************************************
;*                          data area                                *
;*********************************************************************
.err:       db          "ROM-ERR", 0
GDT_value:  dw          rom_end-GDT_value       ; value / null descriptor
            dd          510h
            dw          0
            dd          0000FFFFh,00009800h     ;  8 - legacy real cs
            dd          0000FFFFh,00CF9A00h     ; 16 - prot mode cs
            dd          0000FFFFh,008F9200h     ; 24 - prot mode ds
            dd          0000FFFFh,00AF9A00h     ; 32 - long mode cs
            dd          0000FFFFh,00CF9200h     ; 40 - long mode ds
rom_end:
