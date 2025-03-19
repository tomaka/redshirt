;
;  src/romfoss_x86.asm
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
;  @brief FOSSBIOS boot ROM for Simpleboot, written with flatassembler:
;      https://flatassembler.net It's job is to extract loader_x86.efi's
;      sections from the ROM buffer and pass control to it
;
;  Memory layout on handover:
;        0h -  1000h    stack (2k+)
;     8000h - 60000h    loader_x86.efi's sections
;
; FOSSBIOS is a 64-bit long mode BIOS which uses SysV ABI so passes
; a pointer to the FOSSBIOS Main Structure to functions in rdi

simpleboot_addr equ     08000h

            ORG         0h
            USE64

; FOSSBIOS Main Structure
virtual at 0
fossbios.magic:   dd    ?
fossbios.arch:    dw    ?
fossbios.wordsize:db    ?
fossbios.endian:  db    ?
fossbios.oem:     dq    ?
fossbios.conf:    dq    ?
fossbios.system:  dq    ?
fossbios.proc:    dq    ?
fossbios.video:   dq    ?
fossbios.storage: dq    ?
fossbios.serial:  dq    ?
fossbios.input:   dq    ?
fossbios.clock:   dq    ?
fossbios.audio:   dq    ?
fossbios.net:     dq    ?
fossbios.power:   dq    ?
end virtual

; FOSSBIOS System Services
virtual at 0
system.magic:     dd    ?
system.nument:    dd    ?
system.memmap:    dq    ?
system.boot:      dq    ?
end virtual

;*********************************************************************
;*                             code                                  *
;*********************************************************************
init:       ;---- initialization ----
            ; find our function
            call        @f
@@:         pop         rax
            add         rax, boot-@b
            ; set "system->boot()" hook
            mov         rdi, qword [rdi + fossbios.system]
            mov         qword [rdi + system.boot], rax
            xor         rax, rax
            xor         rdx, rdx
            ret

boot:       ;---- boot procedure ----
            mov         r10, rdi
            xor         rsp, rsp
            mov         sp, 1000h               ; set stack
            ; find our data
            call        @f
@@:         pop         rbx
            add         rbx, payload-@b
            xor         rax, rax
            mov         ax, simpleboot_addr
            ;---- parse loader_x86.efi (PE / COFF format) ----
            mov         r8, rbx
            xor         r9, r9
            xor         rdi, rdi
            add         rbx, qword [rbx + 0x3c] ; get COFF header
            mov         dl, byte [rbx + 6]      ; number of sections
            mov         r9d, dword [rbx + 0x28] ; entry point
            mov         ebp, dword [rbx + 0x2c] ; code base
            add         r9d, eax
            sub         r9d, ebp
            add         bx, word [rbx + 0x14]   ; add header size
            add         bx, 24                  ; ebx now points to section table
@@:         mov         edi, dword [rbx + 12]   ; copy sections from PE into VA
            add         edi, eax
            sub         edi, ebp                ; dest: virtual address + reloc offset - code base
            mov         ecx, dword [rbx + 16]   ; size of raw data
            mov         esi, dword [rbx + 20]
            add         rsi, r8                 ; source: pointer to raw data + load offset
            repnz       movsb
            add         rbx, 40                 ; go to next section
            dec         dl
            jnz         @b
            ;---- set FOSSBIOS Boot Loader arguments ----
            ; rdi: magic
            ; rsi: pointer to FOSSBIOS Main Structure
            ; rdx: boot device index
            xor         rdx, rdx
            mov         rsi, r10
            xor         rdi, rdi
            mov         edi, 0F055B105h
            jmp         r9                      ; jump to relocated entry point
payload:
