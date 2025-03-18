;
;  src/cdemu_x86.asm
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
;  @brief Legacy mode BIOS CDROM boot for Simpleboot, written with
;      flatassembler: https://flatassembler.net It's job is to hook on INT 13h,
;      then load and execute the original MBR boot code as usual
;
;  Memory layout on handover:
;        0h -   400h    IVT (must be preserved)
;      400h -  7C00h    stack
;     7C00h -  7E00h    we're loaded here, also original MBR loaded here
;    9A000h - 9A200h    our relocated TSR code
;    9A200h - 9AA00h    CDROM sector cache

SEG         equ         09A00h
            ORG         0h
            USE16

virtual at 0
lbapacket.size: dw      ?
lbapacket.count:dw      ?
lbapacket.addr0:dw      ?
lbapacket.addr1:dw      ?
lbapacket.sect0:dw      ?
lbapacket.sect1:dw      ?
lbapacket.sect2:dw      ?
lbapacket.sect3:dw      ?
end virtual

;*********************************************************************
;*                          data area                                *
;*********************************************************************
            ;could be 0:7C00 or 07C0:0 as well depending on BIOS
lbapacket:  jmp         short @f                ; mandatory jump (magic)
            nop
            db          13 dup 0
origip:     dw          0
origcs:     dw          0
save:       dd          0
drive:      dw          0
            db          060h-($-$$) dup 0       ; skip over FAT32 BPB

;*********************************************************************
;*                             code                                  *
;*********************************************************************
@@:         cli
            cld
            xor         ax, ax
            mov         ss, ax
            mov         sp, 07C00h
            ;find our position in memory.
            push        cs
            pop         ds
            call        @f
@@:         pop         si
            sub         si, @b-lbapacket
            ;---- relocate ourself to 09A00:0000 ----
            mov         ax, SEG
            mov         es, ax
            xor         di, di
            mov         cx, 200h
            repnz       movsw
            push        es
            pop         ds
            jmp         SEG:.start
            ;clear and reuse BPB data area
.start:     xor         di, di
            xor         ax, ax
            mov         cx, 30h
            repnz       stosw
            mov         byte [drive], dl        ; save CDROM's drive code
            xor         di, di
            mov         al, 16                  ; .size
            stosw
            mov         al, 1                   ; .count
            stosw
            mov         ax, cache               ; .addr0
            push        ax
            stosw
            mov         ax, es                  ; .addr1
            stosw
            ;load the original MBR (plus 3 more sectors) into cache
            mov         ah, byte 42h
            xor         si, si
            int         13h
            ;copy the first 512 bytes of cache to 0:7C00h
            pop         si
            mov         di, sp
            push        es
            xor         ax, ax
            mov         es, ax
            mov         cx, 100h
            repnz       movsw
            pop         es
            ;---- install TSR, hook INT 13h ----
            mov         di, origip
            xor         ax, ax
            mov         ds, ax
            mov         si, 13h * 4
            mov         ax, word[si]
            stosw
            mov         word[si], tsr
            add         si, 2
            mov         ax, word[si]
            stosw
            mov         ax, es
            mov         word[si], ax
            ;---- arrange environment and jump to MBR code ----
            xor         ax, ax
            mov         es, ax
            mov         bx, ax
            mov         cx, ax
            mov         dx, ax
            mov         si, ax
            mov         di, ax
            mov         dl, 81h                 ; secondary BIOS HDD
            mov         ax, 0AA55h
            jmp         0:7C00h

            ;---- emulate 2048 bytes CDROM sectors as 512 bytes HDD sectors ----
tsr:        ;pass through all non "extended read on secondary HDD" calls
            cmp         ah, 42h
            jne         @f
            cmp         dl, 81h
            je          .emulate
@@:         jmp         dword[cs:origip]
.emulate:   push        es
            push        ds
            push        cx
            push        dx
            push        si
            ;read the lba packet we're servicing into registers
            mov         di, word [si + lbapacket.addr0]
            mov         ax, word [si + lbapacket.addr1]
            mov         es, ax
            xor         bh, bh
            mov         bl, byte [si + lbapacket.count]
            mov         edx, dword [si + lbapacket.sect0]
            push        cs
            pop         ds
            or          bl, bl
            jz          .none
            ;---- for each sector ----
.next:      ;do we have the relevant 2048 bytes sector cached?
            mov         eax, edx
            shr         eax, 2
            cmp         eax, dword[lbapacket.sect0]
            je          @f
            ;no, read it in from CDROM
            push        es
            push        ds
            push        di
            push        bx
            mov         dword[save], edx
            ;call original ISR
            mov         dword[lbapacket.sect0], eax
            mov         dl, byte[drive]
            mov         ax, 4200h
            xor         si, si
            clc
            pushf
            call        dword[origip]
            mov         edx, dword[save]
            pop         bx
            pop         di
            pop         ds
            pop         es
            jc          .end
            or          ah, ah
            jnz         .end
            ;copy 512 bytes from cache to caller's buffer
@@:         mov         si, dx
            and         si, 3
            shl         si, 9
            add         si, cache
            mov         cx, 100h
            repnz       movsw
            ;adjust to next sector
            inc         edx
            inc         bh
            dec         bl
            or          bl, bl
            jnz         .next
            ;---- finished ----
.none:      xor         ax, ax
            clc
.end:       pop         si
            pop         dx
            pop         cx
            pop         ds
            pop         es
            ;update the sector count in caller's lba packet
            mov         byte[si + lbapacket.count], bh
            mov         bx, 0AA55h
            iret

            ;padding (check if our code fits into 510 bytes)
            db          01FEh-($-$$) dup 0
            db          55h, 0AAh               ; mandatory magic bytes

;*********************************************************************
;*                           bss area                                *
;*********************************************************************
cache:      db          2048 dup ?              ; CDROM sector cache
