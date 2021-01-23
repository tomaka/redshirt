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

/// Initializes interrupts handling.
pub unsafe fn init() -> Interrupts {
    let value = trap_handler as unsafe extern "C" fn() as usize;
    assert_eq!(value % 4, 0);
    // The 2 less significant bits defined the mode. We keep 0, which means that all
    // exceptions/interrupts go to the same handler.
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

/// Main interrupt handler.
#[cfg(target_pointer_width = "32")]
#[naked]
unsafe extern "C" fn trap_handler() {
    // It is important for all the registers to be restored to their previous values at the end
    // of the interrupt handler. The code below saves all the caller-saved registers according to
    // the RISC-V calling conventions.
    // TODO: what about float registers?
    asm!(r#"
    .align 4
        addi sp, sp, -64

        sw x1, 0(sp)
        sw x5, 4(sp)
        sw x6, 8(sp)
        sw x7, 12(sp)
        sw x10, 16(sp)
        sw x11, 20(sp)
        sw x12, 24(sp)
        sw x13, 28(sp)
        sw x14, 32(sp)
        sw x15, 36(sp)
        sw x16, 40(sp)
        sw x17, 44(sp)
        sw x28, 48(sp)
        sw x29, 52(sp)
        sw x30, 56(sp)
        sw x31, 60(sp)

        jal ra, {trap_handler_rust}

        lw x1, 0(sp)
        lw x5, 4(sp)
        lw x6, 8(sp)
        lw x7, 12(sp)
        lw x10, 16(sp)
        lw x11, 20(sp)
        lw x12, 24(sp)
        lw x13, 28(sp)
        lw x14, 32(sp)
        lw x15, 36(sp)
        lw x16, 40(sp)
        lw x17, 44(sp)
        lw x28, 48(sp)
        lw x29, 52(sp)
        lw x30, 56(sp)
        lw x31, 60(sp)

        addi sp, sp, 64
        mret
    "#,
        trap_handler_rust = sym trap_handler_rust,
        options(noreturn));
}

/// Equivalent to the `trap_handler` above, for 64bits.
#[cfg(target_pointer_width = "64")]
#[naked]
unsafe extern "C" fn trap_handler() {
    // TODO: what about float registers?
    asm!(r#"
    .align 4
        addi sp, sp, -128

        sd x1, 0(sp)
        sd x5, 8(sp)
        sd x6, 16(sp)
        sd x7, 24(sp)
        sd x10, 32(sp)
        sd x11, 40(sp)
        sd x12, 48(sp)
        sd x13, 56(sp)
        sd x14, 64(sp)
        sd x15, 72(sp)
        sd x16, 80(sp)
        sd x17, 88(sp)
        sd x28, 96(sp)
        sd x29, 104(sp)
        sd x30, 112(sp)
        sd x31, 120(sp)

        jal ra, {trap_handler_rust}

        ld x1, 0(sp)
        ld x5, 8(sp)
        ld x6, 16(sp)
        ld x7, 24(sp)
        ld x10, 32(sp)
        ld x11, 40(sp)
        ld x12, 48(sp)
        ld x13, 56(sp)
        ld x14, 64(sp)
        ld x15, 72(sp)
        ld x16, 80(sp)
        ld x17, 88(sp)
        ld x28, 96(sp)
        ld x29, 104(sp)
        ld x30, 112(sp)
        ld x31, 120(sp)

        addi sp, sp, 128
        mret
    "#,
        trap_handler_rust = sym trap_handler_rust,
        options(noreturn));
}

/// Called on interrupt after all registers have been saved on the stack.
///
/// The function is marked as `extern "C"` in order to be sure that it respects the C
/// calling conventions.
unsafe extern "C" fn trap_handler_rust() {
    let mcause: usize;
    asm!("csrr {}, mcause", out(reg) mcause, options(nomem, nostack, preserves_flags));

    // TODO: the code below is note keeping; depending on the interrupt we must increment mepc, otherwise `mret` will jump back to the same instruction that triggered the interrupt
    let mepc: usize;
    asm!("csrr {}, mepc", out(reg) mepc, options(nomem, nostack, preserves_flags));
    asm!("csrw mepc, {}", in(reg) mepc + 4, options(nomem, nostack, preserves_flags));

    // TODO:
    panic!("Interrupt with mcause = 0x{:x}", mcause);
}
