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

use core::convert::TryFrom as _;

pub struct VbeContext {
    machine: Machine,
    /// First megabyte of memory of the machine. Contains the video BIOS.
    memory: Vec<u8>,
    int10h_seg: u16,
    int10h_ptr: u16,
}

impl VbeContext {
    pub async fn new() -> Self {
        let first_mb = unsafe { redshirt_hardware_interface::read(0x0, 0x100000).await };

        let int10h_seg = u16::from_le_bytes(
            <[u8; 2]>::try_from(&first_mb[(0x10 * 4) + 2..(0x10 * 4) + 4]).unwrap(),
        );
        let int10h_ptr = u16::from_le_bytes(
            <[u8; 2]>::try_from(&first_mb[(0x10 * 4) + 0..(0x10 * 4) + 2]).unwrap(),
        );
        log::trace!("Segment = 0x{:x}, pointer = 0x{:x}", int10h_seg, int10h_ptr);

        let machine = Machine {
            regs: Registers {
                eax: 0x4f00, // TODO: hack
                ecx: 0,
                edx: 0,
                ebx: 0x113,  // TODO: hack
                esp: 0xf000, // TODO:
                ebp: 0,
                esi: 0,
                edi: 0,
                eip: u32::from(int10h_ptr),
                cs: 0,
                ss: 0x9000, // TODO:
                ds: 0,
                es: 0x50,
                fs: 0,
                gs: 0,
                flags: 0b0000000000000010,
            },
        };

        VbeContext {
            memory: first_mb,
            machine,
            int10h_seg,
            int10h_ptr,
        }
    }

    pub fn call(&mut self) {
        let mut decoder = iced_x86::Decoder::new(16, &self.memory, iced_x86::DecoderOptions::NONE);

        self.machine.stack_push(&self.machine.regs.flags.to_le_bytes());
        self.machine.stack_push(&self.machine.regs.cs.to_le_bytes());
        self.machine.stack_push(
            &u16::try_from(self.machine.regs.eip & 0xffff)
                .unwrap()
                .to_le_bytes(),
        );

        self.machine.regs.cs = self.int10h_seg;
        self.machine.regs.eip = u32::from(self.int10h_ptr);

        self.machine.write_memory(0x500, &b"VBE2"[..]);

        let mut instr_counter: u32 = 0;

        loop {
            instr_counter = instr_counter.wrapping_add(1);
            if (instr_counter % 1000) == 0 {
                log::info!("Executed 1000 instructions");
            }

            let rip = (u64::from(self.machine.regs.cs) << 4) + u64::from(self.machine.regs.eip);
            assert!(usize::try_from(rip).unwrap() < self.memory.len());
            decoder.set_position(usize::try_from(rip).unwrap());
            decoder.set_ip(rip);

            let instruction = decoder.decode();
            self.machine.regs.eip += u32::try_from(instruction.len()).unwrap(); // TODO: check segment bounds
            assert_eq!(
                decoder.ip(),
                (u64::from(self.machine.regs.cs) << 4) + u64::from(self.machine.regs.eip)
            );

            assert!(!instruction.has_xrelease_prefix());

            match instruction.code() {
                iced_x86::Code::Add_rm32_imm8
                | iced_x86::Code::Add_rm32_imm32
                | iced_x86::Code::Add_rm32_r32
                | iced_x86::Code::Add_r32_rm32 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 4];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let (result, overflow) =
                        u32::from_le_bytes(val1).overflowing_add(u32::from_le_bytes(val2));
                    self.machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    self.machine.flags_set_sign((result & 0x80) != 0);
                    self.machine.flags_set_zero(result == 0);
                    self.machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::And_rm8_imm8
                | iced_x86::Code::And_rm8_r8
                | iced_x86::Code::And_r8_rm8
                | iced_x86::Code::And_AL_imm8 => {
                    let mut val1 = [0; 1];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 1];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let temp = val1[0] & val2[0];
                    self.machine.store_in_operand(&instruction, 0, &temp.to_le_bytes());
                    self.machine.flags_set_sign((temp & 0x80) != 0);
                    self.machine.flags_set_zero(temp == 0);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine.flags_set_carry(false);
                    self.machine.flags_set_overflow(false);
                }

                iced_x86::Code::Call_rel16 => {
                    self.machine.stack_push(
                        &u16::try_from(self.machine.regs.eip & 0xffff)
                            .unwrap()
                            .to_le_bytes(),
                    );
                    self.machine.apply_rel_jump(&instruction);
                }

                iced_x86::Code::Cld => self.machine.flags_set_direction(false),
                iced_x86::Code::Cli => self.machine.flags_set_interrupt(false),

                iced_x86::Code::Cmp_AL_imm8
                | iced_x86::Code::Cmp_r8_rm8
                | iced_x86::Code::Cmp_rm8_r8
                | iced_x86::Code::Cmp_rm8_imm8 => {
                    let mut val1 = [0; 1];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 1];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let (result, overflow) =
                        u8::from_le_bytes(val1).overflowing_sub(u8::from_le_bytes(val2));
                    self.machine.flags_set_sign((result & 0x80) != 0);
                    self.machine.flags_set_zero(result == 0);
                    self.machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Cmp_r16_rm16
                | iced_x86::Code::Cmp_rm16_r16
                | iced_x86::Code::Cmp_rm16_imm8 => {
                    let mut val1 = [0; 2];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 2];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let (result, overflow) =
                        u16::from_le_bytes(val1).overflowing_sub(u16::from_le_bytes(val2));
                    self.machine.flags_set_sign((result & 0x80) != 0);
                    self.machine.flags_set_zero(result == 0);
                    self.machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Cmp_rm32_r32
                | iced_x86::Code::Cmp_rm32_imm8
                | iced_x86::Code::Cmp_rm32_imm32 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 4];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let (result, overflow) =
                        u32::from_le_bytes(val1).overflowing_sub(u32::from_le_bytes(val2));
                    self.machine.flags_set_sign((result & 0x80) != 0);
                    self.machine.flags_set_zero(result == 0);
                    self.machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Dec_r16 | iced_x86::Code::Dec_r32 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let (result, overflow) = u32::from_le_bytes(val1).overflowing_sub(1);
                    // TODO: check flags correctness
                    self.machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    self.machine.flags_set_sign((result & 0x80) != 0);
                    self.machine.flags_set_zero(result == 0);
                    self.machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Imul_r32_rm32_imm8 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 4];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);
                    let mut val3 = [0; 4];
                    self.machine.get_operand_value(&instruction, 2, &mut val3);

                    let (result, overflow) =
                        u32::from_le_bytes(val2).overflowing_add(u32::from_le_bytes(val3));
                    self.machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    // TODO: check flags
                    self.machine.flags_set_sign((result & 0x80) != 0);
                    self.machine.flags_set_zero(result == 0);
                    self.machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Inc_r16 | iced_x86::Code::Inc_r32 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let (result, overflow) = u32::from_le_bytes(val1).overflowing_add(1);
                    // TODO: check flags correctness
                    self.machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    self.machine.flags_set_sign((result & 0x80) != 0);
                    self.machine.flags_set_zero(result == 0);
                    self.machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Iretw => {
                    log::info!("Success!");
                    break;
                }

                iced_x86::Code::Ja_rel8_16 => {
                    if !self.machine.flags_is_carry() && !self.machine.flags_is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Jb_rel8_16 => {
                    if self.machine.flags_is_carry() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Jbe_rel8_16 => {
                    if self.machine.flags_is_carry() || self.machine.flags_is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Je_rel8_16 => {
                    if self.machine.flags_is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Je_rel16 => {
                    if self.machine.flags_is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Jg_rel8_16 => {
                    if !self.machine.flags_is_zero() && !self.machine.flags_is_sign() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Jle_rel8_16 => {
                    if self.machine.flags_is_zero() && !self.machine.flags_is_sign() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Jmp_rel8_16 | iced_x86::Code::Jmp_rel16 => {
                    self.machine.apply_rel_jump(&instruction);
                }
                iced_x86::Code::Jne_rel8_16 => {
                    if !self.machine.flags_is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Jne_rel16 => {
                    if !self.machine.flags_is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }

                iced_x86::Code::Lea_r16_m => {
                    let addr = self.machine.memory_operand_address_no_segment(&instruction, 1);
                    self.machine.store_in_operand(&instruction, 0, &addr.to_le_bytes());
                }

                iced_x86::Code::Mov_r8_imm8
                | iced_x86::Code::Mov_rm8_imm8
                | iced_x86::Code::Mov_r8_rm8
                | iced_x86::Code::Mov_rm8_r8 => {
                    let mut out = [0; 1];
                    self.machine.get_operand_value(&instruction, 1, &mut out);
                    self.machine.store_in_operand(&instruction, 0, &out);
                }

                iced_x86::Code::Mov_r16_imm16
                | iced_x86::Code::Mov_rm16_imm16
                | iced_x86::Code::Mov_r16_rm16
                | iced_x86::Code::Mov_rm16_r16
                | iced_x86::Code::Mov_rm16_Sreg
                | iced_x86::Code::Mov_Sreg_rm16 => {
                    let mut out = [0; 2];
                    self.machine.get_operand_value(&instruction, 1, &mut out);
                    self.machine.store_in_operand(&instruction, 0, &out);
                }

                iced_x86::Code::Mov_r32_imm32
                | iced_x86::Code::Mov_r32_rm32
                | iced_x86::Code::Mov_rm32_r32
                | iced_x86::Code::Mov_EAX_moffs32 => {
                    let mut out = [0; 4];
                    self.machine.get_operand_value(&instruction, 1, &mut out);
                    self.machine.store_in_operand(&instruction, 0, &out);
                }

                iced_x86::Code::Movzx_r32_rm16 => {
                    let mut out = [0; 2];
                    self.machine.get_operand_value(&instruction, 1, &mut out);
                    let zero_extended = [out[0], out[1], 0, 0];
                    self.machine.store_in_operand(&instruction, 0, &zero_extended);
                }

                iced_x86::Code::Nopd => {}
                iced_x86::Code::Nopq => {}
                iced_x86::Code::Nopw => {}

                iced_x86::Code::Or_rm32_imm8
                | iced_x86::Code::Or_rm32_imm32
                | iced_x86::Code::Or_rm32_r32
                | iced_x86::Code::Or_r32_rm32 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 4];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let temp: u32 = u32::from_le_bytes(val1) | u32::from_le_bytes(val2);
                    self.machine.store_in_operand(&instruction, 0, &temp.to_le_bytes());
                    // TODO: flags might not be correct
                    self.machine.flags_set_sign((temp & 0x80) != 0);
                    self.machine.flags_set_zero(temp == 0);
                    self.machine.flags_set_parity_from_val(temp.to_le_bytes()[0]);
                    self.machine.flags_set_carry(false);
                    self.machine.flags_set_overflow(false);
                }

                iced_x86::Code::Out_imm8_AL | iced_x86::Code::Out_DX_AL => {
                    assert!(!instruction.has_rep_prefix()); // TODO: not supported
                    let mut port = [0; 2];
                    self.machine.get_operand_value(&instruction, 0, &mut port);
                    let mut data = [0; 1];
                    self.machine.get_operand_value(&instruction, 1, &mut data);
                    unsafe {
                        redshirt_hardware_interface::port_write_u8(
                            u32::from(u16::from_le_bytes(port)),
                            u8::from_le_bytes(data),
                        );
                    }
                }
                iced_x86::Code::Outsb_DX_m8 => {
                    assert!(!instruction.has_rep_prefix()); // TODO: not supported
                    let mut port = [0; 2];
                    self.machine.get_operand_value(&instruction, 0, &mut port);
                    let mut data = [0; 1];
                    self.machine.get_operand_value(&instruction, 1, &mut data);
                    unsafe {
                        redshirt_hardware_interface::port_write_u8(
                            u32::from(u16::from_le_bytes(port)),
                            u8::from_le_bytes(data),
                        );
                    }
                    if self.machine.flags_is_direction() {
                        self.machine.regs.esi = self.machine.regs.esi.wrapping_sub(1);
                    } else {
                        self.machine.regs.esi = self.machine.regs.esi.wrapping_add(1);
                    }
                }

                iced_x86::Code::Pop_r16 | iced_x86::Code::Pop_rm16 | iced_x86::Code::Popw_DS => {
                    let mut out = [0; 2];
                    self.machine.stack_pop(&mut out);
                    self.machine.store_in_operand(&instruction, 0, &out);
                }
                iced_x86::Code::Pop_r32 | iced_x86::Code::Pop_rm32 => {
                    let mut out = [0; 4];
                    self.machine.stack_pop(&mut out);
                    self.machine.store_in_operand(&instruction, 0, &out);
                }
                iced_x86::Code::Popfw => {
                    let mut out = [0; 2];
                    self.machine.stack_pop(&mut out);
                    self.machine.regs.flags = u16::from_le_bytes(out);
                    // TODO: ensure correctness
                }

                iced_x86::Code::Push_r16 | iced_x86::Code::Push_rm16 | iced_x86::Code::Pushw_DS => {
                    let mut out = [0; 2];
                    self.machine.get_operand_value(&instruction, 0, &mut out);
                    self.machine.stack_push(&out);
                }
                iced_x86::Code::Pushd_imm32
                | iced_x86::Code::Push_r32
                | iced_x86::Code::Push_rm32 => {
                    let mut out = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut out);
                    self.machine.stack_push(&out);
                }
                iced_x86::Code::Pushfw => {
                    self.machine.stack_push(&self.machine.regs.flags.to_le_bytes());
                }

                iced_x86::Code::Retnw => {
                    let mut ip_ret = [0; 2];
                    self.machine.stack_pop(&mut ip_ret);
                    self.machine.regs.eip = u32::from(u16::from_le_bytes(ip_ret));
                }
                iced_x86::Code::Retnw_imm16 => {
                    let mut num_to_pop = [0; 2];
                    self.machine.get_operand_value(&instruction, 0, &mut num_to_pop);
                    let mut ip_ret = [0; 2];
                    self.machine.stack_pop(&mut ip_ret);
                    for _ in 0..u16::from_le_bytes(num_to_pop) {
                        let mut dummy = [0; 1];
                        self.machine.stack_pop(&mut dummy);
                    }
                    self.machine.regs.eip = u32::from(u16::from_le_bytes(ip_ret));
                }

                iced_x86::Code::Shl_rm32_imm8 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 1];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let result =
                        u32::from_le_bytes(val1).wrapping_shl(u32::from(u8::from_le_bytes(val2)));
                    self.machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    // TODO: clusterfuck of eflags
                    /*
                    If the count is 1 or greater, the CF flag is filled with the last bit shifted out of the destination operand and the SF, ZF,
                    and PF flags are set according to the value of the result. For a 1-bit shift, the OF flag is set if a sign change occurred;
                    otherwise, it is cleared. For shifts greater than 1 bit, the OF flag is undefined. If a shift occurs, the AF flag is unde-
                    fined. If the count operand is 0, the flags are not affected. If the count is greater than the operand size, the flags
                    are undefined.*/
                }

                iced_x86::Code::Shr_rm32_imm8 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 1];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let result =
                        u32::from_le_bytes(val1).wrapping_shr(u32::from(u8::from_le_bytes(val2)));
                    self.machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    // TODO: clusterfuck of eflags
                    /*
                    If the count is 1 or greater, the CF flag is filled with the last bit shifted out of the destination operand and the SF, ZF,
                    and PF flags are set according to the value of the result. For a 1-bit shift, the OF flag is set if a sign change occurred;
                    otherwise, it is cleared. For shifts greater than 1 bit, the OF flag is undefined. If a shift occurs, the AF flag is unde-
                    fined. If the count operand is 0, the flags are not affected. If the count is greater than the operand size, the flags
                    are undefined.*/
                }

                iced_x86::Code::Std => self.machine.flags_set_direction(true),
                iced_x86::Code::Sti => self.machine.flags_set_interrupt(true),

                iced_x86::Code::Stosb_m8_AL => {
                    let mut val = [0; 1];
                    self.machine.get_operand_value(&instruction, 1, &mut val);
                    if !instruction.has_rep_prefix() {
                        self.machine.store_in_operand(&instruction, 0, &val);
                        if self.machine.flags_is_direction() {
                            self.machine.regs.edi = self.machine.regs.edi.wrapping_sub(1);
                        } else {
                            self.machine.regs.edi = self.machine.regs.edi.wrapping_add(1);
                        }
                    } else {
                        while self.machine.regs.ecx != 0 {
                            self.machine.store_in_operand(&instruction, 0, &val);
                            if self.machine.flags_is_direction() {
                                self.machine.regs.edi = self.machine.regs.edi.wrapping_sub(1);
                            } else {
                                self.machine.regs.edi = self.machine.regs.edi.wrapping_add(1);
                            }
                            self.machine.regs.ecx -= 1;
                        }
                    }
                }

                iced_x86::Code::Sub_AX_imm16 => {
                    let mut val1 = [0; 2];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 2];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let (result, overflow) =
                        u16::from_le_bytes(val1).overflowing_sub(u16::from_le_bytes(val2));
                    self.machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    self.machine.flags_set_sign((result & 0x80) != 0);
                    self.machine.flags_set_zero(result == 0);
                    self.machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Sub_rm32_imm8 | iced_x86::Code::Sub_rm32_r32 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 4];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let (result, overflow) =
                        u32::from_le_bytes(val1).overflowing_sub(u32::from_le_bytes(val2));
                    self.machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    self.machine.flags_set_sign((result & 0x80) != 0);
                    self.machine.flags_set_zero(result == 0);
                    self.machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Test_rm8_imm8 | iced_x86::Code::Test_rm8_r8 => {
                    let mut val1 = [0; 1];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 1];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let temp = val1[0] & val2[0];
                    self.machine.flags_set_sign((temp & 0x80) != 0);
                    self.machine.flags_set_zero(temp == 0);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine.flags_set_carry(false);
                    self.machine.flags_set_overflow(false);
                }

                iced_x86::Code::Test_rm32_r32 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 4];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let temp = u32::from_le_bytes(val1) & u32::from_le_bytes(val2);
                    self.machine.flags_set_sign((temp & 0x80) != 0);
                    self.machine.flags_set_zero(temp == 0);
                    self.machine.flags_set_parity_from_val(temp.to_le_bytes()[0]);
                    self.machine.flags_set_carry(false);
                    self.machine.flags_set_overflow(false);
                }

                iced_x86::Code::Xor_rm32_r32 | iced_x86::Code::Xor_r32_rm32 => {
                    let mut val1 = [0; 4];
                    self.machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 4];
                    self.machine.get_operand_value(&instruction, 1, &mut val2);

                    let temp = u32::from_le_bytes(val1) ^ u32::from_le_bytes(val2);
                    self.machine.store_in_operand(&instruction, 0, &temp.to_le_bytes());
                    self.machine.flags_set_sign((temp & 0x80) != 0);
                    self.machine.flags_set_zero(temp == 0);
                    self.machine.flags_set_parity_from_val(temp.to_le_bytes()[0]);
                    self.machine.flags_set_carry(false);
                    self.machine.flags_set_overflow(false);
                }

                _ => {
                    log::error!("Unsupported instruction: {:?}", instruction.code());
                    break;
                }
            }
        }

        log::info!("EAX after VBE call: 0x{:x}", self.machine.regs.eax);
        let mut sig = [0; 512];
        self.machine.read_memory(0x500, &mut sig[..]);
        log::info!("Signature: {:?}", &sig[..]);

        let mut oem_ptr_seg = [0; 2];
        self.machine.read_memory(0x508, &mut oem_ptr_seg[..]);
        let mut oem_ptr = [0; 2];
        self.machine.read_memory(0x506, &mut oem_ptr[..]);
        let oem_ptr = (u32::from(u16::from_le_bytes(oem_ptr_seg)) << 4) + u32::from(u16::from_le_bytes(oem_ptr));
        let mut str_out = vec![0; 32];
        self.machine.read_memory(oem_ptr, &mut str_out);
        let len = str_out.iter().position(|b| *b == 0).unwrap_or(str_out.len());
        log::info!("OEM string: {:?}", core::str::from_utf8(&str_out[..len]));
    }
}

pub struct Machine {
    regs: Registers,
}

impl Machine {
    fn read_memory(&self, addr: u32, out: &mut [u8]) {
        let out_len = u32::try_from(out.len()).unwrap();
        assert!(addr + out_len <= 0x100000);

        // TODO: asyncify?
        redshirt_syscalls::block_on(async move {
            unsafe {
                redshirt_hardware_interface::read_to(u64::from(addr), out).await;
            }
        });
    }

    fn write_memory(&mut self, addr: u32, data: &[u8]) {
        let data_len = u32::try_from(data.len()).unwrap();
        assert!(addr + data_len <= 0x100000);

        // TODO: detect if we overwrite the program and reload the decoder
        // TODO: the VBE docs say that only I/O port operations are used

        unsafe {
            redshirt_hardware_interface::write(u64::from(addr), data);
        }
    }

    fn apply_rel_jump(&mut self, instruction: &iced_x86::Instruction) {
        // TODO: check segment bounds
        // TODO: this function's usefulness is debatable; it exists because I didn't realize that near_branch16() automatically calculated the target
        self.regs.eip = u32::from(instruction.near_branch16());
    }

    /// Pushes data on the stack.
    fn stack_push(&mut self, data: &[u8]) {
        // TODO: don't panic
        self.regs.esp = self
            .regs
            .esp
            .checked_sub(u32::try_from(data.len()).unwrap())
            .unwrap();
        let addr = (u32::from(self.regs.ss) << 4) + self.regs.esp;
        self.write_memory(addr, data);
    }

    /// Pops data from the stack.
    fn stack_pop(&mut self, out: &mut [u8]) {
        let addr = (u32::from(self.regs.ss) << 4) + self.regs.esp;
        self.read_memory(addr, out);
        // TODO: don't panic
        self.regs.esp = self
            .regs
            .esp
            .checked_add(u32::try_from(out.len()).unwrap())
            .unwrap();
    }

    fn flags_is_carry(&self) -> bool {
        (self.regs.flags & 1 << 0) != 0
    }

    fn flags_set_carry(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 0;
        } else {
            self.regs.flags &= !(1 << 0);
        }
    }

    fn flags_set_parity(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 2;
        } else {
            self.regs.flags &= !(1 << 2);
        }
    }

    fn flags_set_parity_from_val(&mut self, val: u8) {
        self.flags_set_parity((val.count_ones() % 2) == 0);
    }

    fn flags_is_zero(&self) -> bool {
        (self.regs.flags & 1 << 6) != 0
    }

    fn flags_set_zero(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 6;
        } else {
            self.regs.flags &= !(1 << 6);
        }
    }

    fn flags_is_sign(&self) -> bool {
        (self.regs.flags & 1 << 7) != 0
    }

    fn flags_set_sign(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 7;
        } else {
            self.regs.flags &= !(1 << 7);
        }
    }

    fn flags_set_interrupt(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 9;
        } else {
            self.regs.flags &= !(1 << 9);
        }
    }

    fn flags_is_direction(&self) -> bool {
        (self.regs.flags & 1 << 10) != 0
    }

    fn flags_set_direction(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 10;
        } else {
            self.regs.flags &= !(1 << 10);
        }
    }

    fn flags_set_overflow(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 11;
        } else {
            self.regs.flags &= !(1 << 11);
        }
    }

    fn memory_operand_address_no_segment(
        &self,
        instruction: &iced_x86::Instruction,
        op_n: u32,
    ) -> u16 {
        assert!(matches!(
            instruction.op_kind(op_n),
            iced_x86::OpKind::Memory
        ));

        let base = u16::try_from(
            match instruction.memory_base() {
                iced_x86::Register::None => 0,
                reg => {
                    let mut out = [0; 4];
                    self.get_register(reg, &mut out[..reg.size()]);
                    u32::from_le_bytes(out)
                }
            } & 0xffff,
        )
        .unwrap();

        let index = u16::try_from(
            match instruction.memory_index() {
                iced_x86::Register::None => 0,
                reg => {
                    let mut out = [0; 4];
                    self.get_register(reg, &mut out[..reg.size()]);
                    u32::from_le_bytes(out)
                }
            } & 0xffff,
        )
        .unwrap();

        let index_scale = u16::try_from(instruction.memory_index_scale()).unwrap();

        let base_and_index = base.wrapping_add(index.wrapping_mul(index_scale));

        if instruction.memory_displacement() >= 0x800 {
            // Negative number.
        } else {
        }
        // TODO: wrong?
        base_and_index
            .wrapping_add(u16::try_from(instruction.memory_displacement() & 0xffff).unwrap())
    }

    fn memory_operand_address(&self, instruction: &iced_x86::Instruction, op_n: u32) -> u32 {
        let base = u32::from(self.memory_operand_address_no_segment(instruction, op_n));

        let segment = u32::from({
            let mut out = [0; 2];
            self.get_register(instruction.memory_segment(), &mut out);
            u16::from_le_bytes(out)
        });

        (segment << 4) + base
    }

    fn get_operand_value(&self, instruction: &iced_x86::Instruction, op_n: u32, out: &mut [u8]) {
        match instruction.op_kind(op_n) {
            iced_x86::OpKind::Register => self.get_register(instruction.op_register(op_n), out),
            iced_x86::OpKind::Immediate8 => {
                out.copy_from_slice(&instruction.immediate8().to_le_bytes())
            }
            iced_x86::OpKind::Immediate16 => {
                out.copy_from_slice(&instruction.immediate16().to_le_bytes())
            }
            iced_x86::OpKind::Immediate32 => {
                out.copy_from_slice(&instruction.immediate32().to_le_bytes())
            }
            iced_x86::OpKind::Immediate64 => {
                out.copy_from_slice(&instruction.immediate64().to_le_bytes())
            }
            iced_x86::OpKind::Immediate8to16 => {
                out.copy_from_slice(&instruction.immediate8to16().to_le_bytes())
            }
            iced_x86::OpKind::Immediate8to32 => {
                out.copy_from_slice(&instruction.immediate8to32().to_le_bytes())
            }
            iced_x86::OpKind::Immediate8to64 => {
                out.copy_from_slice(&instruction.immediate8to64().to_le_bytes())
            }
            iced_x86::OpKind::Immediate32to64 => {
                out.copy_from_slice(&instruction.immediate32to64().to_le_bytes())
            }
            iced_x86::OpKind::MemorySegSI => {
                let mut segment_base = [0; 2];
                self.get_register(instruction.memory_segment(), &mut segment_base);
                let addr =
                    (u32::from(u16::from_le_bytes(segment_base)) << 4) + (self.regs.esi & 0xffff);
                self.read_memory(addr, out);
            }
            iced_x86::OpKind::Memory => {
                let addr = self.memory_operand_address(instruction, op_n);
                self.read_memory(addr, out);
            }
            ty => unimplemented!("{:?}", ty),
        }
    }

    fn get_register(&self, register: iced_x86::Register, out: &mut [u8]) {
        match register {
            iced_x86::Register::AL => out.copy_from_slice(&self.regs.eax.to_le_bytes()[..1]),
            iced_x86::Register::CL => out.copy_from_slice(&self.regs.ecx.to_le_bytes()[..1]),
            iced_x86::Register::DL => out.copy_from_slice(&self.regs.edx.to_le_bytes()[..1]),
            iced_x86::Register::BL => out.copy_from_slice(&self.regs.ebx.to_le_bytes()[..1]),
            iced_x86::Register::AH => out.copy_from_slice(&self.regs.eax.to_le_bytes()[1..2]),
            iced_x86::Register::CH => out.copy_from_slice(&self.regs.ecx.to_le_bytes()[1..2]),
            iced_x86::Register::DH => out.copy_from_slice(&self.regs.edx.to_le_bytes()[1..2]),
            iced_x86::Register::BH => out.copy_from_slice(&self.regs.ebx.to_le_bytes()[1..2]),
            iced_x86::Register::AX => out.copy_from_slice(&self.regs.eax.to_le_bytes()[..2]),
            iced_x86::Register::CX => out.copy_from_slice(&self.regs.ecx.to_le_bytes()[..2]),
            iced_x86::Register::DX => out.copy_from_slice(&self.regs.edx.to_le_bytes()[..2]),
            iced_x86::Register::BX => out.copy_from_slice(&self.regs.ebx.to_le_bytes()[..2]),
            iced_x86::Register::SP => out.copy_from_slice(&self.regs.esp.to_le_bytes()[..2]),
            iced_x86::Register::BP => out.copy_from_slice(&self.regs.ebp.to_le_bytes()[..2]),
            iced_x86::Register::SI => out.copy_from_slice(&self.regs.esi.to_le_bytes()[..2]),
            iced_x86::Register::DI => out.copy_from_slice(&self.regs.edi.to_le_bytes()[..2]),
            iced_x86::Register::EAX => out.copy_from_slice(&self.regs.eax.to_le_bytes()),
            iced_x86::Register::ECX => out.copy_from_slice(&self.regs.ecx.to_le_bytes()),
            iced_x86::Register::EDX => out.copy_from_slice(&self.regs.edx.to_le_bytes()),
            iced_x86::Register::EBX => out.copy_from_slice(&self.regs.ebx.to_le_bytes()),
            iced_x86::Register::ESP => out.copy_from_slice(&self.regs.esp.to_le_bytes()),
            iced_x86::Register::EBP => out.copy_from_slice(&self.regs.ebp.to_le_bytes()),
            iced_x86::Register::ESI => out.copy_from_slice(&self.regs.esi.to_le_bytes()),
            iced_x86::Register::EDI => out.copy_from_slice(&self.regs.edi.to_le_bytes()),
            iced_x86::Register::ES => out.copy_from_slice(&self.regs.es.to_le_bytes()),
            iced_x86::Register::CS => out.copy_from_slice(&self.regs.cs.to_le_bytes()),
            iced_x86::Register::SS => out.copy_from_slice(&self.regs.ss.to_le_bytes()),
            iced_x86::Register::DS => out.copy_from_slice(&self.regs.ds.to_le_bytes()),
            iced_x86::Register::FS => out.copy_from_slice(&self.regs.fs.to_le_bytes()),
            iced_x86::Register::GS => out.copy_from_slice(&self.regs.gs.to_le_bytes()),
            reg => unimplemented!("{:?}", reg),
        }
    }

    fn store_in_operand(&mut self, instruction: &iced_x86::Instruction, op_n: u32, val: &[u8]) {
        match instruction.op_kind(op_n) {
            iced_x86::OpKind::Register => {
                self.store_in_register(instruction.op_register(op_n), val)
            }
            iced_x86::OpKind::MemoryESDI => {
                let addr = (u32::from(self.regs.es) << 4) + (self.regs.edi & 0xffff);
                self.write_memory(addr, val);
            }
            iced_x86::OpKind::Memory => {
                let addr = self.memory_operand_address(instruction, op_n);
                self.write_memory(addr, val);
            }
            ty => unimplemented!("{:?}", ty),
        }
    }

    fn store_in_register(&mut self, register: iced_x86::Register, val: &[u8]) {
        match register {
            iced_x86::Register::AL => {
                assert_eq!(val.len(), 1);
                self.regs.eax &= 0xffffff00;
                self.regs.eax |= u32::from(val[0]);
            }
            iced_x86::Register::CL => {
                assert_eq!(val.len(), 1);
                self.regs.ecx &= 0xffffff00;
                self.regs.ecx |= u32::from(val[0]);
            }
            iced_x86::Register::DL => {
                assert_eq!(val.len(), 1);
                self.regs.edx &= 0xffffff00;
                self.regs.edx |= u32::from(val[0]);
            }
            iced_x86::Register::BL => {
                assert_eq!(val.len(), 1);
                self.regs.ebx &= 0xffffff00;
                self.regs.ebx |= u32::from(val[0]);
            }
            iced_x86::Register::AH => {
                assert_eq!(val.len(), 1);
                self.regs.eax &= 0xffff00ff;
                self.regs.eax |= u32::from(val[0]) << 4;
            }
            iced_x86::Register::CH => {
                assert_eq!(val.len(), 1);
                self.regs.ecx &= 0xffff00ff;
                self.regs.ecx |= u32::from(val[0]) << 4;
            }
            iced_x86::Register::DH => {
                assert_eq!(val.len(), 1);
                self.regs.edx &= 0xffff00ff;
                self.regs.edx |= u32::from(val[0]) << 4;
            }
            iced_x86::Register::BH => {
                assert_eq!(val.len(), 1);
                self.regs.ebx &= 0xffff00ff;
                self.regs.ebx |= u32::from(val[0]) << 4;
            }
            iced_x86::Register::AX => {
                self.regs.eax &= 0xffff0000;
                self.regs.eax |= u32::from(u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap()));
            }
            iced_x86::Register::CX => {
                self.regs.ecx &= 0xffff0000;
                self.regs.ecx |= u32::from(u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap()));
            }
            iced_x86::Register::DX => {
                self.regs.edx &= 0xffff0000;
                self.regs.edx |= u32::from(u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap()));
            }
            iced_x86::Register::BX => {
                self.regs.ebx &= 0xffff0000;
                self.regs.ebx |= u32::from(u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap()));
            }
            iced_x86::Register::SP => {
                self.regs.esp &= 0xffff0000;
                self.regs.esp |= u32::from(u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap()));
            }
            iced_x86::Register::BP => {
                self.regs.ebp &= 0xffff0000;
                self.regs.ebp |= u32::from(u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap()));
            }
            iced_x86::Register::SI => {
                self.regs.esi &= 0xffff0000;
                self.regs.esi |= u32::from(u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap()));
            }
            iced_x86::Register::DI => {
                self.regs.edi &= 0xffff0000;
                self.regs.edi |= u32::from(u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap()));
            }
            iced_x86::Register::EAX => {
                self.regs.eax = u32::from_le_bytes(<[u8; 4]>::try_from(val).unwrap())
            }
            iced_x86::Register::ECX => {
                self.regs.ecx = u32::from_le_bytes(<[u8; 4]>::try_from(val).unwrap())
            }
            iced_x86::Register::EDX => {
                self.regs.edx = u32::from_le_bytes(<[u8; 4]>::try_from(val).unwrap())
            }
            iced_x86::Register::EBX => {
                self.regs.ebx = u32::from_le_bytes(<[u8; 4]>::try_from(val).unwrap())
            }
            iced_x86::Register::ESP => {
                self.regs.esp = u32::from_le_bytes(<[u8; 4]>::try_from(val).unwrap())
            }
            iced_x86::Register::EBP => {
                self.regs.ebp = u32::from_le_bytes(<[u8; 4]>::try_from(val).unwrap())
            }
            iced_x86::Register::ESI => {
                self.regs.esi = u32::from_le_bytes(<[u8; 4]>::try_from(val).unwrap())
            }
            iced_x86::Register::EDI => {
                self.regs.edi = u32::from_le_bytes(<[u8; 4]>::try_from(val).unwrap())
            }
            iced_x86::Register::ES => {
                self.regs.es = u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap())
            }
            iced_x86::Register::CS => {
                self.regs.cs = u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap())
            }
            iced_x86::Register::SS => {
                self.regs.ss = u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap())
            }
            iced_x86::Register::DS => {
                self.regs.ds = u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap())
            }
            iced_x86::Register::FS => {
                self.regs.fs = u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap())
            }
            iced_x86::Register::GS => {
                self.regs.gs = u16::from_le_bytes(<[u8; 2]>::try_from(val).unwrap())
            }
            reg => unimplemented!("{:?}", reg),
        }
    }
}

pub struct Registers {
    eax: u32,
    ecx: u32,
    edx: u32,
    ebx: u32,
    esp: u32,
    ebp: u32,
    esi: u32,
    edi: u32,
    eip: u32,
    cs: u16,
    ss: u16,
    ds: u16,
    es: u16,
    fs: u16,
    gs: u16,
    flags: u16,
}
