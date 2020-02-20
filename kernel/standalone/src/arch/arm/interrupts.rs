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

//! Interrupts management.
//!
//! When an interrupt happens on an ARM platform, the CPU automatically sets the PC register
//! (Program Counter) to a certain value depending on the interrupt. For example, executing
//! an illegal instruction makes the CPU jump to the address 0x4. The CPU then continues
//! execution at this location.
//!
//! In order to properly handle interrupts, we write the memory locations where the CPU can
//! potentially jump to with branching instructions.

#[cold]
fn setup_vector_table() {
    
}

#[cold]
// TODO: do the aarch64 version
#[cfg(target_arch = "arm")]
fn gen_branch_opcode(branch_instr_loc: u32, branch_target: u32) -> u32 {
    // See chapter "A8.8.18  B" of the ARM® Architecture Reference Manual (ARMv7-A and
    // ARMv7-R edition).
    //
    // We use the A1 encoding of the branch instruction (`B`) with the condition "Always".
    // This is encoded with the 8 highest bits being 0b11101010 (0xEA), and the 24 bits lowest
    // bits being an immediate value.
    // This immediate value is an offset relative to the PC when the branch is executed, divided
    // by four. The immediate is a signed integer, meaning that we only have 23 bits at our
    // disposal.
    // TODO: Keep in mind that instructions are encoded in little endian.

    // I'm not sure that the instruction below is correct in situations where we jump back. Since
    // in practice we never jump back, let's decide to not support this.
    assert!(branch_instr_loc <= branch_target);

    // Note: this ` - 2` was determined empirically.
    let imm = ((branch_target - branch_instr_loc) >> 2).checked_sub(2).unwrap();
    if imm >= 0x800000 {
        panic!();       // TODO: should do a `ldr pc, some_loc` where `some_loc` contains the actual pointer
    }

    0xea000000 | imm
}
