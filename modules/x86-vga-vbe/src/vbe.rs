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

pub struct VbeContext {}

impl VbeContext {
    pub async fn new() {
        let first_mb = unsafe { redshirt_hardware_interface::read(0x0, 0x100000).await };

        log::trace!("Read first megabyte");

        let int10h_seg = u16::from_le_bytes(
            <[u8; 2]>::try_from(&first_mb[(0x10 * 4) + 2..(0x10 * 4) + 4]).unwrap(),
        );
        let int10h_ptr = u16::from_le_bytes(
            <[u8; 2]>::try_from(&first_mb[(0x10 * 4) + 0..(0x10 * 4) + 2]).unwrap(),
        );
        log::trace!("Segment = 0x{:x}, pointer = 0x{:x}", int10h_seg, int10h_ptr);

        let mut decoder = iced_x86::Decoder::new(16, &first_mb, iced_x86::DecoderOptions::NONE);

        let mut machine = Machine {
            memory: first_mb.clone(),
            regs: Registers {
                eax: 0x4f00,
                ecx: 0,
                edx: 0,
                ebx: 0,
                esp: 0xf000, // TODO:
                ebp: 0,
                esi: 0,
                edi: 0,
                eip: u32::from(int10h_ptr),
                cs: 0,
                ss: 0xf000, // TODO:
                ds: 0,
                es: 0,
                fs: 0,
                gs: 0,
                flags: 0b0000000000000010,
            },
        };

        machine.stack_push(&machine.regs.flags.to_le_bytes());
        machine.stack_push(&machine.regs.cs.to_le_bytes());
        machine.stack_push(
            &u16::try_from(machine.regs.eip & 0xffff)
                .unwrap()
                .to_le_bytes(),
        );

        machine.regs.cs = int10h_seg;
        machine.regs.eip = u32::from(int10h_ptr);

        loop {
            let rip = (u64::from(machine.regs.cs) << 4) + u64::from(machine.regs.eip);
            assert!(usize::try_from(rip).unwrap() < first_mb.len());
            decoder.set_position(usize::try_from(rip).unwrap());
            decoder.set_ip(rip);

            let instruction = decoder.decode();
            //assert!(!instruction.is_privileged());
            machine.regs.eip += u32::try_from(instruction.len()).unwrap(); // TODO: check segment bounds
            assert_eq!(
                decoder.ip(),
                (u64::from(machine.regs.cs) << 4) + u64::from(machine.regs.eip)
            );

            log::trace!("Instruction = {:?}", instruction.code());

            assert!(!instruction.has_xrelease_prefix());

            match instruction.code() {
                iced_x86::Code::And_rm8_imm8
                | iced_x86::Code::And_rm8_r8
                | iced_x86::Code::And_r8_rm8
                | iced_x86::Code::And_AL_imm8 => {
                    let mut val1 = [0; 1];
                    machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 1];
                    machine.get_operand_value(&instruction, 1, &mut val2);

                    let temp = val1[0] & val2[0];
                    machine.store_in_operand(&instruction, 0, &temp.to_le_bytes());
                    machine.flags_set_sign((temp & 0x80) != 0);
                    machine.flags_set_zero(temp == 0);
                    machine.flags_set_parity_from_val(temp);
                    machine.flags_set_carry(false);
                    machine.flags_set_overflow(false);
                }

                iced_x86::Code::Call_rel16 => {
                    machine.stack_push(
                        &u16::try_from(machine.regs.eip & 0xffff)
                            .unwrap()
                            .to_le_bytes(),
                    );
                    machine.apply_rel_jump(&instruction);
                }

                iced_x86::Code::Cld => machine.flags_set_direction(false),
                iced_x86::Code::Cli => machine.flags_set_interrupt(false),

                iced_x86::Code::Cmp_AL_imm8 |
                iced_x86::Code::Cmp_r8_rm8 |
                iced_x86::Code::Cmp_rm8_r8 |
                iced_x86::Code::Cmp_rm8_imm8  => {
                    let mut val1 = [0; 1];
                    machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 1];
                    machine.get_operand_value(&instruction, 1, &mut val2);

                    let (result, overflow) =
                        u8::from_le_bytes(val1).overflowing_sub(u8::from_le_bytes(val2));
                    machine.flags_set_sign((result & 0x80) != 0);
                    machine.flags_set_zero(result == 0);
                    machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    machine.flags_set_carry(overflow);
                    machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Ja_rel8_16 => {
                    if !machine.flags_is_carry() && !machine.flags_is_zero() {
                        machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Jb_rel8_16 => {
                    if machine.flags_is_carry() {
                        machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Je_rel8_16 => {
                    if machine.flags_is_zero() {
                        machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Je_rel16 => {
                    if machine.flags_is_zero() {
                        machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Jmp_rel16 => {
                    machine.apply_rel_jump(&instruction);
                }
                iced_x86::Code::Jne_rel8_16 => {
                    if !machine.flags_is_zero() {
                        machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Code::Jne_rel16 => {
                    if !machine.flags_is_zero() {
                        machine.apply_rel_jump(&instruction);
                    }
                }

                iced_x86::Code::Lea_r16_m => {
                    let addr = machine.memory_operand_address_no_segment(&instruction, 1);
                    machine.store_in_operand(&instruction, 0, &addr.to_le_bytes());
                }

                iced_x86::Code::Mov_r8_imm8
                | iced_x86::Code::Mov_r8_rm8
                | iced_x86::Code::Mov_rm8_r8 => {
                    let mut out = [0; 1];
                    machine.get_operand_value(&instruction, 1, &mut out);
                    machine.store_in_operand(&instruction, 0, &out);
                }

                iced_x86::Code::Mov_r16_imm16
                | iced_x86::Code::Mov_r16_rm16
                | iced_x86::Code::Mov_rm16_r16
                | iced_x86::Code::Mov_rm16_Sreg
                | iced_x86::Code::Mov_Sreg_rm16 => {
                    let mut out = [0; 2];
                    machine.get_operand_value(&instruction, 1, &mut out);
                    machine.store_in_operand(&instruction, 0, &out);
                }

                iced_x86::Code::Mov_r32_imm32
                | iced_x86::Code::Mov_r32_rm32
                | iced_x86::Code::Mov_rm32_r32
                | iced_x86::Code::Mov_EAX_moffs32 => {
                    let mut out = [0; 4];
                    machine.get_operand_value(&instruction, 1, &mut out);
                    machine.store_in_operand(&instruction, 0, &out);
                }

                iced_x86::Code::Movzx_r32_rm16 => {
                    let mut out = [0; 2];
                    machine.get_operand_value(&instruction, 1, &mut out);
                    let zero_extended = [out[0], out[1], 0, 0];
                    machine.store_in_operand(&instruction, 0, &zero_extended);
                }

                iced_x86::Code::Nopd => {}
                iced_x86::Code::Nopq => {}
                iced_x86::Code::Nopw => {}

                iced_x86::Code::Or_rm32_imm32 |
                iced_x86::Code::Or_rm32_r32 |
                iced_x86::Code::Or_r32_rm32 => {
                    let mut val1 = [0; 4];
                    machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 4];
                    machine.get_operand_value(&instruction, 1, &mut val2);

                    let temp: u32 = u32::from_le_bytes(val1) | u32::from_le_bytes(val2);
                    machine.store_in_operand(&instruction, 0, &temp.to_le_bytes());
                    // TODO: flags might not be correct
                    machine.flags_set_sign((temp & 0x80) != 0);
                    machine.flags_set_zero(temp == 0);
                    machine.flags_set_parity_from_val(temp.to_le_bytes()[0]);
                    machine.flags_set_carry(false);
                    machine.flags_set_overflow(false);
                }

                iced_x86::Code::Outsb_DX_m8 => {
                    let mut port = [0; 2];
                    machine.get_operand_value(&instruction, 0, &mut port);
                    let mut data = [0; 1];
                    machine.get_operand_value(&instruction, 1, &mut data);
                    unsafe {
                        redshirt_hardware_interface::port_write_u8(
                            u32::from(u16::from_le_bytes(port)),
                            u8::from_le_bytes(data),
                        );
                    }
                    if machine.flags_is_direction() {
                        machine.regs.esi = machine.regs.esi.wrapping_sub(1);
                    } else {
                        machine.regs.esi = machine.regs.esi.wrapping_add(1);
                    }
                }

                iced_x86::Code::Pop_r16 | iced_x86::Code::Pop_rm16 | iced_x86::Code::Popw_DS => {
                    let mut out = [0; 2];
                    machine.stack_pop(&mut out);
                    machine.store_in_operand(&instruction, 0, &out);
                }
                iced_x86::Code::Pop_r32 | iced_x86::Code::Pop_rm32 => {
                    let mut out = [0; 4];
                    machine.stack_pop(&mut out);
                    machine.store_in_operand(&instruction, 0, &out);
                }

                iced_x86::Code::Push_r16 |
                iced_x86::Code::Pushw_DS => {
                    let mut out = [0; 2];
                    machine.get_operand_value(&instruction, 0, &mut out);
                    machine.stack_push(&out);
                }
                iced_x86::Code::Push_r32 => {
                    let mut out = [0; 4];
                    machine.get_operand_value(&instruction, 0, &mut out);
                    machine.stack_push(&out);
                }
                iced_x86::Code::Pushfw => {
                    machine.stack_push(&machine.regs.flags.to_le_bytes());
                }

                iced_x86::Code::Shl_rm32_imm8 => {
                    let mut val1 = [0; 4];
                    machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 1];
                    machine.get_operand_value(&instruction, 1, &mut val2);

                    let result = u32::from_le_bytes(val1).wrapping_shl(u32::from(u8::from_le_bytes(val2)));
                    machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    // TODO: clusterfuck of eflags
                    /*
                    If the count is 1 or greater, the CF flag is filled with the last bit shifted out of the destination operand and the SF, ZF,
                    and PF flags are set according to the value of the result. For a 1-bit shift, the OF flag is set if a sign change occurred;
                    otherwise, it is cleared. For shifts greater than 1 bit, the OF flag is undefined. If a shift occurs, the AF flag is unde-
                    fined. If the count operand is 0, the flags are not affected. If the count is greater than the operand size, the flags
                    are undefined.*/
                }

                iced_x86::Code::Std => machine.flags_set_direction(true),
                iced_x86::Code::Sti => machine.flags_set_interrupt(true),

                iced_x86::Code::Stosb_m8_AL => {
                    let mut val = [0; 1];
                    machine.get_operand_value(&instruction, 1, &mut val);
                    if !instruction.has_rep_prefix() {
                        machine.store_in_operand(&instruction, 0, &val);
                        if machine.flags_is_direction() {
                            machine.regs.edi = machine.regs.edi.wrapping_sub(1);
                        } else {
                            machine.regs.edi = machine.regs.edi.wrapping_add(1);
                        }
                    } else {
                        while machine.regs.ecx != 0 {
                            machine.store_in_operand(&instruction, 0, &val);
                            if machine.flags_is_direction() {
                                machine.regs.edi = machine.regs.edi.wrapping_sub(1);
                            } else {
                                machine.regs.edi = machine.regs.edi.wrapping_add(1);
                            }
                            machine.regs.ecx -= 1;
                        }
                    }
                }

                iced_x86::Code::Sub_AX_imm16 => {
                    let mut val1 = [0; 2];
                    machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 2];
                    machine.get_operand_value(&instruction, 1, &mut val2);

                    let (result, overflow) =
                        u16::from_le_bytes(val1).overflowing_sub(u16::from_le_bytes(val2));
                    machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    machine.flags_set_sign((result & 0x80) != 0);
                    machine.flags_set_zero(result == 0);
                    machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    machine.flags_set_carry(overflow);
                    machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Sub_rm32_imm8 => {
                    let mut val1 = [0; 4];
                    machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 4];
                    machine.get_operand_value(&instruction, 1, &mut val2);

                    let (result, overflow) =
                        u32::from_le_bytes(val1).overflowing_sub(u32::from_le_bytes(val2));
                    machine.store_in_operand(&instruction, 0, &result.to_le_bytes());
                    machine.flags_set_sign((result & 0x80) != 0);
                    machine.flags_set_zero(result == 0);
                    machine.flags_set_parity_from_val(result.to_le_bytes()[0]);
                    machine.flags_set_carry(overflow);
                    machine.flags_set_overflow(overflow); // FIXME: this is wrong but I don't understand
                                                          // FIXME: set AF flag
                }

                iced_x86::Code::Test_rm8_imm8 |
                iced_x86::Code::Test_rm8_r8 => {
                    let mut val1 = [0; 1];
                    machine.get_operand_value(&instruction, 0, &mut val1);
                    let mut val2 = [0; 1];
                    machine.get_operand_value(&instruction, 1, &mut val2);

                    let temp = val1[0] & val2[0];
                    machine.flags_set_sign((temp & 0x80) != 0);
                    machine.flags_set_zero(temp == 0);
                    machine.flags_set_parity_from_val(temp);
                    machine.flags_set_carry(false);
                    machine.flags_set_overflow(false);
                }

                _ => {
                    log::error!("Unsupported instruction: {:?}", instruction.code());
                    break;
                }
            }
        }
    }
}

pub struct Machine {
    memory: Vec<u8>,
    regs: Registers,
}

impl Machine {
    fn apply_rel_jump(&mut self, instruction: &iced_x86::Instruction) {
        // TODO: check segment bounds
        // TODO: this function's usefulness is debatable; it exists because I didn't realize that near_branch16() automatically calculated the target
        self.regs.eip = u32::from(instruction.near_branch16());
        log::trace!("Jumped to 0x{:4x}:0x{:4x}", self.regs.cs, self.regs.eip);
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
        let addr_usize = usize::try_from(addr).unwrap();
        self.memory[addr_usize..addr_usize + data.len()].copy_from_slice(data);
    }

    /// Pops data from the stack.
    fn stack_pop(&mut self, out: &mut [u8]) {
        let addr = (u32::from(self.regs.ss) << 4) + self.regs.esp;
        let addr_usize = usize::try_from(addr).unwrap();
        assert!(addr_usize + out.len() <= self.memory.len());
        out.copy_from_slice(&self.memory[addr_usize..addr_usize + out.len()]);
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

    fn memory_operand_address_no_segment(&self, instruction: &iced_x86::Instruction, op_n: u32) -> u16 {
        assert!(matches!(
            instruction.op_kind(op_n),
            iced_x86::OpKind::Memory
        ));

        let base = u16::try_from(match instruction.memory_base() {
            iced_x86::Register::None => 0,
            reg => {
                let mut out = [0; 4];
                self.get_register(reg, &mut out[..reg.size()]);
                u32::from_le_bytes(out)
            }
        } & 0xffff).unwrap();

        let index = u16::try_from(match instruction.memory_index() {
            iced_x86::Register::None => 0,
            reg => {
                let mut out = [0; 4];
                self.get_register(reg, &mut out[..reg.size()]);
                u32::from_le_bytes(out)
            }
        } & 0xffff).unwrap();

        let index_scale = u16::try_from(instruction.memory_index_scale()).unwrap();

        let base_and_index = base.wrapping_add(index.wrapping_mul(index_scale));
        
        if instruction.memory_displacement() >= 0x800 {
            // Negative number.
        } else {

        }
        // TODO: wrong?
        base_and_index.wrapping_add(u16::try_from(instruction.memory_displacement() & 0xffff).unwrap())
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
                let addr_usize = usize::try_from(addr).unwrap();
                assert!(addr_usize + out.len() <= self.memory.len());
                out.copy_from_slice(&self.memory[addr_usize..addr_usize + out.len()]);
            }
            iced_x86::OpKind::Memory => {
                let addr = usize::try_from(self.memory_operand_address(instruction, op_n)).unwrap();
                assert!(addr + out.len() <= self.memory.len());
                out.copy_from_slice(&self.memory[addr..addr + out.len()]);
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
                let addr_usize = usize::try_from(addr).unwrap();
                assert!(addr_usize + val.len() <= self.memory.len());
                self.memory[addr_usize..addr_usize + val.len()].copy_from_slice(val);
            }
            iced_x86::OpKind::Memory => {
                let addr = self.memory_operand_address(instruction, op_n);
                let addr_usize = usize::try_from(addr).unwrap();
                assert!(addr_usize + val.len() <= self.memory.len());
                self.memory[addr_usize..addr_usize + val.len()].copy_from_slice(val);
                unsafe {
                    redshirt_hardware_interface::write(u64::from(addr), val.to_owned());
                }
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
