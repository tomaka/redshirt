// Copyright (C) 2019-2020  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

.code16
.global _ap_start
.type _ap_start, @function
_ap_start:
    // TODO: remove
    mov $0xb8f0, %ax
    mov %ax, %ds

    mov $0x0, %di
    movb $'H', (%di)
    add $1, %di
    movb $0xf, (%di)

    movb $'e', 0x2
    movb $0xf, 0x3
    movb $'l', 0x4
    movb $0xf, 0x5
    movb $'l', 0x6
    movb $0xf, 0x7
    movb $'o', 0x8
    movb $0xf, 0x9
    cli
    hlt

.global _after_ap_start
.type _after_ap_start, @function
_after_ap_start:
    cli
    hlt
