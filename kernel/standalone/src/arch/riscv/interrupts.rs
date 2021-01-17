// Copyright (C) 2019-2021  Pierre Krieger
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

//! Interrupts handling for RISC-V.

pub unsafe fn init() -> Interrupts {
    let value = trap_handler as unsafe extern "C" fn() as usize;
    assert_eq!(value % 4, 0);
    // The 4 lower bits defined the mode. We keep 0, which means that all exceptions/interrupts
    // go to the same handler.
    asm!("csrw mtvec, {}", in(reg) value, options(nomem, nostack, preserves_flags));
    Interrupts {}
}

pub struct Interrupts {}

impl Drop for Interrupts {
    fn drop(&mut self) {
        // We really don't want that to be destroyed.
        panic!();
    }
}

#[cfg(target_pointer_width = "32")]
#[naked]
unsafe extern "C" fn trap_handler() {
    asm!(r#"
    .align 4
        addi sp, sp, -56

        sw x1, 0(sp)
        sw x5, 2(sp)
        sw x6, 4(sp)
        sw x7, 6(sp)
        sw x8, 8(sp)
        sw x9, 10(sp)
        sw x10, 12(sp)
        sw x11, 14(sp)
        sw x12, 16(sp)
        sw x13, 18(sp)
        sw x14, 20(sp)
        sw x15, 22(sp)
        sw x16, 24(sp)
        sw x17, 26(sp)
        sw x18, 28(sp)
        sw x19, 30(sp)
        sw x20, 32(sp)
        sw x21, 34(sp)
        sw x22, 36(sp)
        sw x23, 38(sp)
        sw x24, 40(sp)
        sw x25, 42(sp)
        sw x26, 44(sp)
        sw x27, 46(sp)
        sw x28, 48(sp)
        sw x29, 50(sp)
        sw x30, 52(sp)
        sw x31, 54(sp)

        jal ra, {trap_handler_rust}

        lw x1, 0(sp)
        lw x5, 2(sp)
        lw x6, 4(sp)
        lw x7, 6(sp)
        lw x8, 8(sp)
        lw x9, 10(sp)
        lw x10, 12(sp)
        lw x11, 14(sp)
        lw x12, 16(sp)
        lw x13, 18(sp)
        lw x14, 20(sp)
        lw x15, 22(sp)
        lw x16, 24(sp)
        lw x17, 26(sp)
        lw x18, 28(sp)
        lw x19, 30(sp)
        lw x20, 32(sp)
        lw x21, 34(sp)
        lw x22, 36(sp)
        lw x23, 38(sp)
        lw x24, 40(sp)
        lw x25, 42(sp)
        lw x26, 44(sp)
        lw x27, 46(sp)
        lw x28, 48(sp)
        lw x29, 50(sp)
        lw x30, 52(sp)
        lw x31, 54(sp)

        addi sp, sp, 56
        mret
    "#,
        trap_handler_rust = sym trap_handler_rust,
        options(noreturn));
}

unsafe extern "C" fn trap_handler_rust() {
    let mcause: usize;
    asm!("csrr {}, mcause", out(reg) mcause, options(nomem, nostack, preserves_flags));
    panic!("Interrupt with mcause = 0x{:x}", mcause);
}
