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

use core::{convert::TryFrom, fmt, mem};

mod tests;

/// Intel 80386 real mode interpreter.
pub struct Interpreter {
    machine: Machine,
    /// First megabyte of memory of the machine. Contains the video BIOS.
    memory: Vec<u8>,
}

#[derive(Debug)]
pub enum Error {
    InvalidInstruction,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::InvalidInstruction => write!(f, "Invalid instruction"),
        }
    }
}

impl Interpreter {
    pub async fn new() -> Self {
        let first_mb = unsafe { redshirt_hardware_interface::read(0x0, 0x100000).await };
        Self::from_memory(first_mb).await
    }

    pub async fn from_memory(first_mb: Vec<u8>) -> Self {
        let machine = Machine {
            local_memory: first_mb.clone(),
            regs: Registers {
                eax: 0,
                ecx: 0,
                edx: 0,
                ebx: 0x113,  // TODO: hack
                esp: 0xf000, // TODO:
                ebp: 0,
                esi: 0,
                edi: 0,
                eip: 0,
                cs: 0,
                ss: 0x9000, // TODO:
                ds: 0,
                es: 0,
                fs: 0,
                gs: 0,
                flags: 0b1011000000000010,
            },
        };

        Interpreter {
            memory: first_mb,
            machine,
        }
    }

    pub fn read_memory_nul_terminated_str(&self, mut addr: u32) -> String {
        let mut out = Vec::new();
        loop {
            match self.read_memory_u8(addr) {
                0 => break,
                b => out.push(b),
            };
            addr += 1;
        }
        String::from_utf8(out).unwrap()
    }

    pub fn read_memory_u8(&self, addr: u32) -> u8 {
        let mut out = [0; 1];
        self.read_memory(addr, &mut out);
        u8::from_le_bytes(out)
    }

    pub fn read_memory_u16(&self, addr: u32) -> u16 {
        let mut out = [0; 2];
        self.read_memory(addr, &mut out);
        u16::from_le_bytes(out)
    }

    pub fn read_memory(&self, addr: u32, out: &mut [u8]) {
        self.machine.read_memory(addr, out)
    }

    pub fn write_memory(&mut self, addr: u32, data: &[u8]) {
        self.machine.write_memory(addr, data);
    }

    pub fn ax(&mut self) -> u16 {
        u16::try_from(self.machine.regs.eax & 0xffff).unwrap()
    }

    pub fn set_ax(&mut self, value: u16) {
        self.machine.regs.eax &= 0xffff0000;
        self.machine.regs.eax |= u32::from(value);
    }

    pub fn set_bx(&mut self, value: u16) {
        self.machine.regs.ebx &= 0xffff0000;
        self.machine.regs.ebx |= u32::from(value);
    }

    pub fn set_cx(&mut self, value: u16) {
        self.machine.regs.ecx &= 0xffff0000;
        self.machine.regs.ecx |= u32::from(value);
    }

    pub fn set_es_di(&mut self, es: u16, di: u16) {
        self.machine.regs.es = es;
        self.machine.regs.edi &= 0xffff0000;
        self.machine.regs.edi |= u32::from(di);
    }

    pub fn int10h(&mut self) -> Result<(), Error> {
        self.machine.int_opcode(0x10);

        let mut decoder = iced_x86::Decoder::new(16, &self.memory, iced_x86::DecoderOptions::NONE);
        let mut instr_counter: u32 = 0;

        loop {
            instr_counter = instr_counter.wrapping_add(1);
            if (instr_counter % 1000) == 0 {
                log::trace!("Executed 1000 instructions");
            }

            // We update the position of the decoder at each loop, to be sure that it is in sync.
            let rip = (u64::from(self.machine.regs.cs) << 4) + u64::from(self.machine.regs.eip);
            assert!(usize::try_from(rip).unwrap() < self.memory.len());
            decoder.set_position(usize::try_from(rip).unwrap());
            decoder.set_ip(rip);

            // Decode instruction and update the IP register.
            let instruction = decoder.decode();
            assert!(!instruction.has_xrelease_prefix());
            self.machine.regs.eip = {
                let ip = u16::try_from(self.machine.regs.eip & 0xffff).unwrap();
                let new_ip = ip.wrapping_add(u16::try_from(instruction.len()).unwrap());
                u32::from(new_ip)
            };

            // List here: https://en.wikipedia.org/wiki/X86_instruction_listings#Original_8086/8088_instructions
            // The objective is to implement up to and including the x386.
            match instruction.mnemonic() {
                iced_x86::Mnemonic::Add => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    let (temp, overflow) = match (value0, value1) {
                        (Value::U8(value0), Value::U8(value1)) => {
                            let (v, o) = value0.overflowing_add(value1);
                            (Value::U8(v), o)
                        }
                        (Value::U16(value0), Value::U16(value1)) => {
                            let (v, o) = value0.overflowing_add(value1);
                            (Value::U16(v), o)
                        }
                        (Value::U32(value0), Value::U32(value1)) => {
                            let (v, o) = value0.overflowing_add(value1);
                            (Value::U32(v), o)
                        }
                        _ => unreachable!(),
                    };

                    self.machine.store_in_operand(&instruction, 0, temp);

                    self.machine.flags_set_sign_from_val(temp);
                    self.machine.flags_set_zero_from_val(temp);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine.flags_set_carry(overflow);
                    self.machine
                        .flags_set_overflow(overflow != temp.left_most_bit());
                    // TODO: the adjust flag
                }

                iced_x86::Mnemonic::And => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    let temp = match (value0, value1) {
                        (Value::U8(value0), Value::U8(value1)) => Value::U8(value0 & value1),
                        (Value::U16(value0), Value::U16(value1)) => Value::U16(value0 & value1),
                        (Value::U32(value0), Value::U32(value1)) => Value::U32(value0 & value1),
                        _ => unreachable!(),
                    };

                    self.machine.store_in_operand(&instruction, 0, temp);

                    self.machine.flags_set_sign_from_val(temp);
                    self.machine.flags_set_zero_from_val(temp);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine.flags_set_carry(false);
                    self.machine.flags_set_overflow(false);
                    // adjust flag is undefined
                }

                iced_x86::Mnemonic::Bt => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    // TODO: might not be correct; there's some weirdness with when it's memory
                    let bit = match (value0, value1) {
                        (Value::U8(value0), Value::U8(value1)) => {
                            (value0 & (1u8.wrapping_shl(u32::from(value1)))) != 0
                        }
                        (Value::U16(value0), Value::U8(value1)) => {
                            (value0 & (1u16.wrapping_shl(u32::from(value1)))) != 0
                        }
                        (Value::U16(value0), Value::U16(value1)) => {
                            (value0 & (1u16.wrapping_shl(u32::from(value1)))) != 0
                        }
                        (Value::U32(value0), Value::U8(value1)) => {
                            (value0 & (1u32.wrapping_shl(u32::from(value1)))) != 0
                        }
                        (Value::U32(value0), Value::U32(value1)) => {
                            (value0 & (1u32.wrapping_shl(u32::from(value1)))) != 0
                        }
                        _ => unreachable!(),
                    };

                    self.machine.flags_set_carry(bit);
                }

                iced_x86::Mnemonic::Call => {
                    match instruction.code() {
                        iced_x86::Code::Call_ptr1616 | iced_x86::Code::Call_m1616 => {
                            let ip = u16::try_from(self.machine.regs.eip & 0xffff).unwrap();
                            self.machine
                                .stack_push_value(Value::U16(self.machine.regs.cs));
                            self.machine.stack_push_value(Value::U16(ip));
                        }
                        iced_x86::Code::Call_rel16 | iced_x86::Code::Call_rm16 => {
                            let ip = u16::try_from(self.machine.regs.eip & 0xffff).unwrap();
                            self.machine.stack_push_value(Value::U16(ip));
                        }
                        _ => unreachable!(),
                    }

                    self.machine.apply_rel_jump(&instruction);
                }

                iced_x86::Mnemonic::Cwd => {
                    if self
                        .machine
                        .register(iced_x86::Register::AX)
                        .left_most_bit()
                    {
                        self.machine
                            .store_in_register(iced_x86::Register::DX, Value::U16(0xffff))
                    } else {
                        self.machine
                            .store_in_register(iced_x86::Register::DX, Value::U16(0x0000))
                    }
                }
                iced_x86::Mnemonic::Cdq => {
                    if self
                        .machine
                        .register(iced_x86::Register::EAX)
                        .left_most_bit()
                    {
                        self.machine
                            .store_in_register(iced_x86::Register::EDX, Value::U32(0xffffffff))
                    } else {
                        self.machine
                            .store_in_register(iced_x86::Register::EDX, Value::U32(0x00000000))
                    }
                }

                iced_x86::Mnemonic::Clc => self.machine.flags_set_carry(false),
                iced_x86::Mnemonic::Cld => self.machine.flags_set_direction(false),
                iced_x86::Mnemonic::Cli => self.machine.flags_set_interrupt(false),

                iced_x86::Mnemonic::Cmp => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    let (temp, overflow) = match (value0, value1) {
                        (Value::U8(value0), Value::U8(value1)) => {
                            let (v, o) = value0.overflowing_sub(value1);
                            (Value::U8(v), o)
                        }
                        (Value::U16(value0), Value::U16(value1)) => {
                            let (v, o) = value0.overflowing_sub(value1);
                            (Value::U16(v), o)
                        }
                        (Value::U32(value0), Value::U32(value1)) => {
                            let (v, o) = value0.overflowing_sub(value1);
                            (Value::U32(v), o)
                        }
                        _ => unreachable!(),
                    };

                    self.machine.flags_set_sign_from_val(temp);
                    self.machine.flags_set_zero_from_val(temp);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine.flags_set_carry(overflow);
                    self.machine
                        .flags_set_overflow(overflow != temp.left_most_bit());
                    // TODO: the adjust flag
                }

                iced_x86::Mnemonic::Dec => {
                    let value = self.machine.fetch_operand_value(&instruction, 0);
                    let (temp, overflow) = match value {
                        Value::U8(value) => {
                            let (v, o) = value.overflowing_sub(1);
                            (Value::U8(v), o)
                        }
                        Value::U16(value) => {
                            let (v, o) = value.overflowing_sub(1);
                            (Value::U16(v), o)
                        }
                        Value::U32(value) => {
                            let (v, o) = value.overflowing_sub(1);
                            (Value::U32(v), o)
                        }
                        _ => unreachable!(),
                    };

                    self.machine.store_in_operand(&instruction, 0, temp);

                    self.machine.flags_set_sign_from_val(temp);
                    self.machine.flags_set_zero_from_val(temp);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine
                        .flags_set_overflow(overflow != temp.left_most_bit());
                    // TODO: the adjust flag
                    // Carry flag is not affected.
                }

                iced_x86::Mnemonic::Div => {
                    // TODO: no check for division by zero
                    match self.machine.fetch_operand_value(&instruction, 0) {
                        Value::U8(divisor) => {
                            let dividend = u16::try_from(self.machine.regs.eax & 0xffff).unwrap();
                            let divisor = u16::from(divisor);
                            let quotient = u8::try_from((dividend / divisor) & 0xff).unwrap();
                            let remainder = u8::try_from(dividend % divisor).unwrap();
                            self.machine
                                .store_in_register(iced_x86::Register::AL, Value::U8(quotient));
                            self.machine
                                .store_in_register(iced_x86::Register::AH, Value::U8(remainder));
                        }
                        Value::U16(divisor) => {
                            let dividend = u32::try_from(
                                ((self.machine.regs.edx & 0xffff) << 16)
                                    | (self.machine.regs.eax & 0xffff),
                            )
                            .unwrap();
                            let divisor = u32::from(divisor);
                            let quotient = u16::try_from((dividend / divisor) & 0xffff).unwrap();
                            let remainder = u16::try_from(dividend % divisor).unwrap();
                            self.machine
                                .store_in_register(iced_x86::Register::AX, Value::U16(quotient));
                            self.machine
                                .store_in_register(iced_x86::Register::DX, Value::U16(remainder));
                        }
                        Value::U32(divisor) => {
                            let dividend = (u64::from(self.machine.regs.edx) << 32)
                                | u64::from(self.machine.regs.eax);
                            let divisor = u64::from(divisor);
                            let quotient =
                                u32::try_from((dividend / divisor) & 0xffffffff).unwrap();
                            let remainder = u32::try_from(dividend % divisor).unwrap();
                            self.machine.regs.eax = quotient;
                            self.machine.regs.edx = remainder;
                        }
                    }
                }

                // TODO: doesn't account for sign; probably wrong
                iced_x86::Mnemonic::Idiv => {
                    // TODO: no check for division by zero
                    match self.machine.fetch_operand_value(&instruction, 0) {
                        Value::U8(divisor) => {
                            let dividend = u16::try_from(self.machine.regs.eax & 0xffff).unwrap();
                            let divisor = u16::from(divisor);
                            let quotient = u8::try_from((dividend / divisor) & 0xff).unwrap();
                            let remainder = u8::try_from(dividend % divisor).unwrap();
                            self.machine
                                .store_in_register(iced_x86::Register::AL, Value::U8(quotient));
                            self.machine
                                .store_in_register(iced_x86::Register::AH, Value::U8(remainder));
                        }
                        Value::U16(divisor) => {
                            let dividend = u32::try_from(
                                ((self.machine.regs.edx & 0xffff) << 16)
                                    | (self.machine.regs.eax & 0xffff),
                            )
                            .unwrap();
                            let divisor = u32::from(divisor);
                            let quotient = u16::try_from((dividend / divisor) & 0xffff).unwrap();
                            let remainder = u16::try_from(dividend % divisor).unwrap();
                            self.machine
                                .store_in_register(iced_x86::Register::AX, Value::U16(quotient));
                            self.machine
                                .store_in_register(iced_x86::Register::DX, Value::U16(remainder));
                        }
                        Value::U32(divisor) => {
                            let dividend = (u64::from(self.machine.regs.edx) << 32)
                                | u64::from(self.machine.regs.eax);
                            let divisor = u64::from(divisor);
                            let quotient =
                                u32::try_from((dividend / divisor) & 0xffffffff).unwrap();
                            let remainder = u32::try_from(dividend % divisor).unwrap();
                            self.machine.regs.eax = quotient;
                            self.machine.regs.edx = remainder;
                        }
                    }
                }

                // TODO: doesn't account for sign; probably wrong
                iced_x86::Mnemonic::Imul if matches!(instruction.op_count(), 1 | 2) => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    let (temp, overflow) = match (value0, value1) {
                        (Value::U8(value0), Value::U8(value1)) => {
                            let (v, o) = value0.overflowing_mul(value1);
                            (Value::U8(v), o)
                        }
                        (Value::U16(value0), Value::U16(value1)) => {
                            let (v, o) = value0.overflowing_mul(value1);
                            (Value::U16(v), o)
                        }
                        (Value::U32(value0), Value::U32(value1)) => {
                            let (v, o) = value0.overflowing_mul(value1);
                            (Value::U32(v), o)
                        }
                        _ => unreachable!(),
                    };

                    self.machine.store_in_operand(&instruction, 0, temp);

                    // TODO: not sure whether this is actually correct for CF and OF;
                    // documentation seems contradictory
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow);
                    // Sign, zero, parity and adjust flags are undefined.
                }

                iced_x86::Mnemonic::Imul if instruction.op_count() == 3 => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);
                    let value2 = self.machine.fetch_operand_value(&instruction, 2);

                    let (temp, overflow) = match (value1, value2) {
                        (Value::U8(value1), Value::U8(value2)) => {
                            let (v, o) = value1.overflowing_mul(value2);
                            (Value::U8(v), o)
                        }
                        (Value::U16(value1), Value::U16(value2)) => {
                            let (v, o) = value1.overflowing_mul(value2);
                            (Value::U16(v), o)
                        }
                        (Value::U32(value1), Value::U32(value2)) => {
                            let (v, o) = value1.overflowing_mul(value2);
                            (Value::U32(v), o)
                        }
                        _ => unreachable!(),
                    };

                    self.machine.store_in_operand(&instruction, 0, temp);

                    // TODO: not sure whether this is actually correct for CF and OF;
                    // documentation seems contradictory
                    self.machine.flags_set_carry(overflow);
                    self.machine.flags_set_overflow(overflow);
                    // Sign, zero, parity and adjust flags are undefined.
                }

                iced_x86::Mnemonic::In => {
                    let port =
                        u16::try_from(self.machine.fetch_operand_value(&instruction, 1)).unwrap();
                    let data = match self.machine.fetch_operand_value(&instruction, 0) {
                        Value::U8(_) => Value::U8(unsafe {
                            redshirt_syscalls::block_on(redshirt_hardware_interface::port_read_u8(
                                u32::from(port),
                            ))
                        }),
                        Value::U16(_) => Value::U16(unsafe {
                            redshirt_syscalls::block_on(redshirt_hardware_interface::port_read_u16(
                                u32::from(port),
                            ))
                        }),
                        Value::U32(_) => Value::U32(unsafe {
                            redshirt_syscalls::block_on(redshirt_hardware_interface::port_read_u32(
                                u32::from(port),
                            ))
                        }),
                    };
                    self.machine.store_in_operand(&instruction, 0, data);
                }

                iced_x86::Mnemonic::Inc => {
                    let value = self.machine.fetch_operand_value(&instruction, 0);
                    let (temp, overflow) = match value {
                        Value::U8(value) => {
                            let (v, o) = value.overflowing_add(1);
                            (Value::U8(v), o)
                        }
                        Value::U16(value) => {
                            let (v, o) = value.overflowing_add(1);
                            (Value::U16(v), o)
                        }
                        Value::U32(value) => {
                            let (v, o) = value.overflowing_add(1);
                            (Value::U32(v), o)
                        }
                        _ => unreachable!(),
                    };

                    self.machine.store_in_operand(&instruction, 0, temp);

                    self.machine.flags_set_sign_from_val(temp);
                    self.machine.flags_set_zero_from_val(temp);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine
                        .flags_set_overflow(overflow != temp.left_most_bit());
                    // TODO: the adjust flag
                    // Carry flag is not affected.
                }

                iced_x86::Mnemonic::Int => {
                    let value = self.machine.fetch_operand_value(&instruction, 0);
                    self.machine.int_opcode(u8::try_from(value).unwrap());
                    log::info!("Int 0x{:?}", value);
                }

                iced_x86::Mnemonic::Iret => {
                    break Ok(());
                }

                iced_x86::Mnemonic::Ja => {
                    if !self.machine.flags_is_carry() && !self.machine.flags_is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jae => {
                    if !self.machine.flags_is_carry() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jb => {
                    if self.machine.flags_is_carry() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jbe => {
                    if self.machine.flags_is_carry() || self.machine.flags_is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jcxz => {
                    if self.machine.regs.ecx & 0xffff == 0 {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Je => {
                    if self.machine.flags_is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jecxz => {
                    if self.machine.regs.ecx == 0 {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jg => {
                    if !self.machine.flags_is_zero() && !self.machine.flags_is_sign() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jge => {
                    if !self.machine.flags_is_sign() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jl => {
                    if self.machine.flags_is_sign() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jle => {
                    if self.machine.flags_is_zero() || self.machine.flags_is_sign() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jmp => {
                    self.machine.apply_rel_jump(&instruction);
                }
                iced_x86::Mnemonic::Jne => {
                    if !self.machine.flags_is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jno => {
                    if !self.machine.flags_is_overflow() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jnp => {
                    if !self.machine.flags_is_parity() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jns => {
                    if !self.machine.flags_is_sign() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jo => {
                    if self.machine.flags_is_overflow() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Jp => {
                    if self.machine.flags_is_parity() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Js => {
                    if self.machine.flags_is_sign() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }

                iced_x86::Mnemonic::Lea => {
                    let ptr = self.machine.memory_operand_pointer(&instruction, 1);
                    self.machine
                        .store_in_operand(&instruction, 0, Value::U16(ptr));
                }

                iced_x86::Mnemonic::Loop => {
                    let cx = self.machine.register(iced_x86::Register::CX);
                    let cx = cx.dec();
                    self.machine.store_in_register(iced_x86::Register::CX, cx);
                    if cx.is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Loope => {
                    let cx = self.machine.register(iced_x86::Register::CX);
                    let cx = cx.dec();
                    self.machine.store_in_register(iced_x86::Register::CX, cx);
                    if cx.is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }
                iced_x86::Mnemonic::Loopne => {
                    let cx = self.machine.register(iced_x86::Register::CX);
                    let cx = cx.dec();
                    self.machine.store_in_register(iced_x86::Register::CX, cx);
                    if !cx.is_zero() {
                        self.machine.apply_rel_jump(&instruction);
                    }
                }

                iced_x86::Mnemonic::Mov => {
                    // TODO: when executing `mov reg, sreg`, the upper bits of `reg` are zeroed
                    // on modern processors; implement properly
                    let value = self.machine.fetch_operand_value(&instruction, 1);
                    self.machine.store_in_operand(&instruction, 0, value);
                }

                iced_x86::Mnemonic::Movzx => {
                    let value = self.machine.fetch_operand_value(&instruction, 1);

                    // We need to figure out the size of the destination.
                    // We implement this in a very lazy and inefficient way by reading its value
                    // in order to determine its size.
                    match (self.machine.fetch_operand_value(&instruction, 0), value) {
                        (Value::U16(_), Value::U8(v)) => {
                            self.machine
                                .store_in_operand(&instruction, 0, Value::U16(u16::from(v)))
                        }
                        (Value::U32(_), Value::U8(v)) => {
                            self.machine
                                .store_in_operand(&instruction, 0, Value::U32(u32::from(v)))
                        }
                        (Value::U32(_), Value::U16(v)) => {
                            self.machine
                                .store_in_operand(&instruction, 0, Value::U32(u32::from(v)))
                        }
                        _ => unreachable!(),
                    }
                }

                iced_x86::Mnemonic::Nop => {}

                iced_x86::Mnemonic::Not => {
                    let value = self.machine.fetch_operand_value(&instruction, 0);
                    let result = match value {
                        Value::U8(value) => Value::U8(value ^ 0xff),
                        Value::U16(value) => Value::U16(value ^ 0xffff),
                        Value::U32(value) => Value::U32(value ^ 0xffffffff),
                        _ => unreachable!(),
                    };
                    self.machine.store_in_operand(&instruction, 0, result);
                }

                iced_x86::Mnemonic::Or => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    let temp = match (value0, value1) {
                        (Value::U8(value0), Value::U8(value1)) => Value::U8(value0 | value1),
                        (Value::U16(value0), Value::U16(value1)) => Value::U16(value0 | value1),
                        (Value::U32(value0), Value::U32(value1)) => Value::U32(value0 | value1),
                        _ => unreachable!(),
                    };

                    self.machine.store_in_operand(&instruction, 0, temp);

                    self.machine.flags_set_sign_from_val(temp);
                    self.machine.flags_set_zero_from_val(temp);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine.flags_set_carry(false);
                    self.machine.flags_set_overflow(false);
                    // adjust flag is undefined
                }

                iced_x86::Mnemonic::Out => {
                    let port =
                        u16::try_from(self.machine.fetch_operand_value(&instruction, 0)).unwrap();
                    match self.machine.fetch_operand_value(&instruction, 1) {
                        Value::U8(data) => unsafe {
                            redshirt_hardware_interface::port_write_u8(u32::from(port), data);
                        },
                        Value::U16(data) => unsafe {
                            redshirt_hardware_interface::port_write_u16(u32::from(port), data);
                        },
                        Value::U32(data) => unsafe {
                            redshirt_hardware_interface::port_write_u32(u32::from(port), data);
                        },
                    }
                }

                iced_x86::Mnemonic::Pop => {
                    // We need to figure out how much data to pop.
                    // We implement this in a very lazy and inefficient way by reading the
                    // location where we will pop to in order to determine its size.
                    match self.machine.fetch_operand_value(&instruction, 0) {
                        Value::U8(_) => {
                            let val = Value::U8(self.machine.stack_pop_u8());
                            self.machine.store_in_operand(&instruction, 0, val);
                        }
                        Value::U16(_) => {
                            let val = Value::U16(self.machine.stack_pop_u16());
                            self.machine.store_in_operand(&instruction, 0, val);
                        }
                        Value::U32(_) => {
                            let val = Value::U32(self.machine.stack_pop_u32());
                            self.machine.store_in_operand(&instruction, 0, val);
                        }
                    }
                }
                iced_x86::Mnemonic::Popa => {
                    match instruction.code() {
                        iced_x86::Code::Popaw => {
                            let val = Value::U16(self.machine.stack_pop_u16());
                            self.machine.store_in_register(iced_x86::Register::DI, val);
                            let val = Value::U16(self.machine.stack_pop_u16());
                            self.machine.store_in_register(iced_x86::Register::SI, val);
                            let val = Value::U16(self.machine.stack_pop_u16());
                            self.machine.store_in_register(iced_x86::Register::BP, val);
                            let _ = self.machine.stack_pop_u16();
                            let val = Value::U16(self.machine.stack_pop_u16());
                            self.machine.store_in_register(iced_x86::Register::BX, val);
                            let val = Value::U16(self.machine.stack_pop_u16());
                            self.machine.store_in_register(iced_x86::Register::DX, val);
                            let val = Value::U16(self.machine.stack_pop_u16());
                            self.machine.store_in_register(iced_x86::Register::CX, val);
                            let val = Value::U16(self.machine.stack_pop_u16());
                            self.machine.store_in_register(iced_x86::Register::AX, val);
                        }
                        iced_x86::Code::Popad => {
                            let val = Value::U32(self.machine.stack_pop_u32());
                            self.machine.store_in_register(iced_x86::Register::EDI, val);
                            let val = Value::U32(self.machine.stack_pop_u32());
                            self.machine.store_in_register(iced_x86::Register::ESI, val);
                            let val = Value::U32(self.machine.stack_pop_u32());
                            self.machine.store_in_register(iced_x86::Register::EBP, val);
                            let _ = self.machine.stack_pop_u32();
                            let val = Value::U32(self.machine.stack_pop_u32());
                            self.machine.store_in_register(iced_x86::Register::EBX, val);
                            let val = Value::U32(self.machine.stack_pop_u32());
                            self.machine.store_in_register(iced_x86::Register::EDX, val);
                            let val = Value::U32(self.machine.stack_pop_u32());
                            self.machine.store_in_register(iced_x86::Register::ECX, val);
                            let val = Value::U32(self.machine.stack_pop_u32());
                            self.machine.store_in_register(iced_x86::Register::EAX, val);
                        }
                        _ => unreachable!()
                    }
                }
                iced_x86::Mnemonic::Popf => {
                    match instruction.code() {
                        iced_x86::Code::Popfw => {
                            let val = self.machine.stack_pop_u16();
                            self.machine.regs.flags = val & 0b0000111111010101;
                        }
                        _ => unimplemented!()
                    }
                }

                iced_x86::Mnemonic::Push => {
                    let value = self.machine.fetch_operand_value(&instruction, 0);
                    self.machine.stack_push_value(value);
                }
                iced_x86::Mnemonic::Pusha => {
                    match instruction.code() {
                        iced_x86::Code::Pushaw => {
                            let sp = self.machine.register(iced_x86::Register::SP);
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::AX));
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::CX));
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::DX));
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::BX));
                            self.machine.stack_push_value(sp);
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::BP));
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::SI));
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::DI));
                        }
                        iced_x86::Code::Pushad => {
                            let esp = self.machine.register(iced_x86::Register::ESP);
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::EAX));
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::ECX));
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::EDX));
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::EBX));
                            self.machine.stack_push_value(esp);
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::EBP));
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::ESI));
                            self.machine.stack_push_value(self.machine.register(iced_x86::Register::EDI));
                        }
                        _ => unreachable!()
                    }
                }
                iced_x86::Mnemonic::Pushf => {
                    match instruction.code() {
                        iced_x86::Code::Pushfw => {
                            self.machine
                                .stack_push_value(Value::U16(self.machine.regs.flags));
                        },
                        _ => unimplemented!()
                    }
                }

                iced_x86::Mnemonic::Ret => {
                    let num_to_pop = if instruction.op_count() == 1 {
                        self.machine
                            .fetch_operand_value(&instruction, 0)
                            .extend_to_u32()
                    } else {
                        0
                    };

                    match instruction.code() {
                        iced_x86::Code::Retnw_imm16 | iced_x86::Code::Retnw => {
                            let ip = self.machine.stack_pop_u16();
                            self.machine.regs.eip = u32::from(ip);
                        }
                        iced_x86::Code::Retfw_imm16 | iced_x86::Code::Retfw => {
                            let ip = self.machine.stack_pop_u16();
                            self.machine.regs.eip = u32::from(ip);
                            let cs = self.machine.stack_pop_u16();
                            self.machine.regs.cs = cs;
                        }
                        _ => unreachable!(),
                    }

                    for _ in 0..num_to_pop {
                        let _ = self.machine.stack_pop_u8();
                    }
                }

                iced_x86::Mnemonic::Sahf => {
                    self.machine
                        .flags_set_sign(self.machine.regs.eax & (1 << 7) != 0);
                    self.machine
                        .flags_set_zero(self.machine.regs.eax & (1 << 6) != 0);
                    self.machine
                        .flags_set_adjust(self.machine.regs.eax & (1 << 4) != 0);
                    self.machine
                        .flags_set_parity(self.machine.regs.eax & (1 << 2) != 0);
                    self.machine
                        .flags_set_carry(self.machine.regs.eax & (1 << 0) != 0);
                }


                iced_x86::Mnemonic::Sal | iced_x86::Mnemonic::Shl => {
                    let mut value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    for _ in 0..value1.extend_to_u32() {
                        let shifted_bit = value0.left_most_bit();

                        value0 = match value0 {
                            Value::U8(v) => Value::U8(v.wrapping_shl(1)),
                            Value::U16(v) => Value::U16(v.wrapping_shl(1)),
                            Value::U32(v) => Value::U32(v.wrapping_shl(1)),
                        };

                        self.machine.flags_set_sign_from_val(value0);
                        self.machine.flags_set_zero_from_val(value0);
                        self.machine.flags_set_parity_from_val(value0);
                        self.machine.flags_set_carry(shifted_bit);
                        self.machine
                            .flags_set_overflow(shifted_bit != value0.left_most_bit());
                        // The adjust flag is undefined
                    }

                    self.machine.store_in_operand(&instruction, 0, value0);
                }

                iced_x86::Mnemonic::Sar | iced_x86::Mnemonic::Shr => {
                    let mut value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    let sign_extension = if let iced_x86::Mnemonic::Sar = instruction.mnemonic() {
                        if value0.left_most_bit() {
                            1u8
                        } else {
                            0u8
                        }
                    } else {
                        0u8
                    };

                    for _ in 0..value1.extend_to_u32() {
                        let shifted_bit = (value0.extend_to_u32() & 0x1) != 0;
                        let sign_bit = value0.left_most_bit();

                        value0 = match value0 {
                            Value::U8(v) => {
                                Value::U8((u8::from(sign_extension) << 7) | v.wrapping_shr(1))
                            }
                            Value::U16(v) => {
                                Value::U16((u16::from(sign_extension) << 15) | v.wrapping_shr(1))
                            }
                            Value::U32(v) => {
                                Value::U32((u32::from(sign_extension) << 31) | v.wrapping_shr(1))
                            }
                        };

                        self.machine.flags_set_sign_from_val(value0);
                        self.machine.flags_set_zero_from_val(value0);
                        self.machine.flags_set_parity_from_val(value0);
                        self.machine.flags_set_carry(shifted_bit);
                        self.machine
                            .flags_set_overflow(sign_bit != value0.left_most_bit());
                        // The adjust flag is undefined
                    }

                    self.machine.store_in_operand(&instruction, 0, value0);
                }

                iced_x86::Mnemonic::Sbb => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    let (temp, overflow) = match (value0, value1) {
                        (Value::U8(value0), Value::U8(value1)) => {
                            let carry_val = if self.machine.flags_is_carry() { 1 } else { 0 };
                            let (v, o) = value0.overflowing_sub(value1.wrapping_add(carry_val));
                            (Value::U8(v), o)
                        }
                        (Value::U16(value0), Value::U16(value1)) => {
                            let carry_val = if self.machine.flags_is_carry() { 1 } else { 0 };
                            let (v, o) = value0.overflowing_sub(value1.wrapping_add(carry_val));
                            (Value::U16(v), o)
                        }
                        (Value::U32(value0), Value::U32(value1)) => {
                            let carry_val = if self.machine.flags_is_carry() { 1 } else { 0 };
                            let (v, o) = value0.overflowing_sub(value1.wrapping_add(carry_val));
                            (Value::U32(v), o)
                        }
                        _ => unreachable!(),
                    };

                    self.machine.store_in_operand(&instruction, 0, temp);

                    self.machine.flags_set_sign_from_val(temp);
                    self.machine.flags_set_zero_from_val(temp);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine.flags_set_carry(overflow);
                    self.machine
                        .flags_set_overflow(overflow != temp.left_most_bit());
                    // TODO: the adjust flag
                }

                iced_x86::Mnemonic::Scasb | iced_x86::Mnemonic::Scasw | iced_x86::Mnemonic::Scasd => {
                    // TODO: this should really be simplified?
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);

                    let counter_reg = match value0 {
                        Value::U8(_) => iced_x86::Register::CX,
                        Value::U16(_) => iced_x86::Register::CX,
                        Value::U32(_) => iced_x86::Register::ECX,
                    };

                    loop {
                        if instruction.has_repe_prefix() || instruction.has_repne_prefix() {
                            if self.machine.register(counter_reg).is_zero() {
                                break;
                            }
                        }

                        let value1 = self.machine.fetch_operand_value(&instruction, 1);

                        let (temp, overflow) = match (value0, value1) {
                            (Value::U8(value0), Value::U8(value1)) => {
                                let (v, o) = value0.overflowing_sub(value1);
                                (Value::U8(v), o)
                            }
                            (Value::U16(value0), Value::U16(value1)) => {
                                let (v, o) = value0.overflowing_sub(value1);
                                (Value::U16(v), o)
                            }
                            (Value::U32(value0), Value::U32(value1)) => {
                                let (v, o) = value0.overflowing_sub(value1);
                                (Value::U32(v), o)
                            }
                            _ => unreachable!(),
                        };

                        self.machine.flags_set_sign_from_val(temp);
                        self.machine.flags_set_zero_from_val(temp);
                        self.machine.flags_set_parity_from_val(temp);
                        self.machine.flags_set_carry(overflow);
                        self.machine
                            .flags_set_overflow(overflow != temp.left_most_bit());
                        // TODO: the adjust flag

                        if self.machine.flags_is_direction() {
                            self.machine.sub_di(u16::from(value0.size()));
                        } else {
                            self.machine.add_di(u16::from(value0.size()));
                        }

                        if instruction.has_repe_prefix() || instruction.has_repne_prefix() {
                            let ecx = self.machine.register(counter_reg);
                            self.machine.store_in_register(counter_reg, ecx.dec());
                        }

                        if instruction.has_repe_prefix() {
                            if !self.machine.flags_is_zero() {
                                break;
                            }
                        } else if instruction.has_repne_prefix() {
                            if self.machine.flags_is_zero() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }

                iced_x86::Mnemonic::Seta => {
                    let value = Value::U8(
                        if !self.machine.flags_is_carry() && !self.machine.flags_is_zero() {
                            1
                        } else {
                            0
                        },
                    );
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setae => {
                    let value = Value::U8(if !self.machine.flags_is_carry() { 1 } else { 0 });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setb => {
                    let value = Value::U8(if self.machine.flags_is_carry() { 1 } else { 0 });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setbe => {
                    let value = Value::U8(
                        if self.machine.flags_is_carry() || self.machine.flags_is_zero() {
                            1
                        } else {
                            0
                        },
                    );
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Sete => {
                    let value = Value::U8(if self.machine.flags_is_zero() { 1 } else { 0 });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setg => {
                    let value = Value::U8(
                        if !self.machine.flags_is_zero() && !self.machine.flags_is_sign() {
                            1
                        } else {
                            0
                        },
                    );
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setge => {
                    let value = Value::U8(if !self.machine.flags_is_sign() { 1 } else { 0 });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setl => {
                    let value = Value::U8(if self.machine.flags_is_sign() { 1 } else { 0 });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setle => {
                    let value = Value::U8(
                        if self.machine.flags_is_zero() || self.machine.flags_is_sign() {
                            1
                        } else {
                            0
                        },
                    );
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setne => {
                    let value = Value::U8(if !self.machine.flags_is_zero() { 1 } else { 0 });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setno => {
                    let value = Value::U8(if !self.machine.flags_is_overflow() {
                        1
                    } else {
                        0
                    });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setnp => {
                    let value = Value::U8(if !self.machine.flags_is_parity() {
                        1
                    } else {
                        0
                    });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setns => {
                    let value = Value::U8(if !self.machine.flags_is_sign() { 1 } else { 0 });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Seto => {
                    let value = Value::U8(if self.machine.flags_is_overflow() {
                        1
                    } else {
                        0
                    });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Setp => {
                    let value = Value::U8(if self.machine.flags_is_parity() { 1 } else { 0 });
                    self.machine.store_in_operand(&instruction, 0, value);
                }
                iced_x86::Mnemonic::Sets => {
                    let value = Value::U8(if self.machine.flags_is_sign() { 1 } else { 0 });
                    self.machine.store_in_operand(&instruction, 0, value);
                }

                iced_x86::Mnemonic::Stc => self.machine.flags_set_carry(true),
                iced_x86::Mnemonic::Std => self.machine.flags_set_direction(true),
                iced_x86::Mnemonic::Sti => self.machine.flags_set_interrupt(true),

                iced_x86::Mnemonic::Stosb => {
                    let val = self.machine.fetch_operand_value(&instruction, 1);

                    if instruction.has_rep_prefix() {
                        while self.machine.regs.ecx & 0xffff != 0 {
                            self.machine.store_in_operand(&instruction, 0, val);
                            if self.machine.flags_is_direction() {
                                self.machine.sub_di(1);
                            } else {
                                self.machine.add_di(1);
                            }
                            self.machine.dec_cx();
                        }
                    } else {
                        self.machine.store_in_operand(&instruction, 0, val);
                        if self.machine.flags_is_direction() {
                            self.machine.sub_di(1);
                        } else {
                            self.machine.add_di(1);
                        }
                    }
                }

                iced_x86::Mnemonic::Sub => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    let (temp, overflow) = match (value0, value1) {
                        (Value::U8(value0), Value::U8(value1)) => {
                            let (v, o) = value0.overflowing_sub(value1);
                            (Value::U8(v), o)
                        }
                        (Value::U16(value0), Value::U16(value1)) => {
                            let (v, o) = value0.overflowing_sub(value1);
                            (Value::U16(v), o)
                        }
                        (Value::U32(value0), Value::U32(value1)) => {
                            let (v, o) = value0.overflowing_sub(value1);
                            (Value::U32(v), o)
                        }
                        _ => unreachable!(),
                    };

                    self.machine.store_in_operand(&instruction, 0, temp);

                    self.machine.flags_set_sign_from_val(temp);
                    self.machine.flags_set_zero_from_val(temp);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine.flags_set_carry(overflow);
                    self.machine
                        .flags_set_overflow(overflow != temp.left_most_bit());
                    // TODO: the adjust flag
                }

                iced_x86::Mnemonic::Test => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    let temp = match (value0, value1) {
                        (Value::U8(value0), Value::U8(value1)) => Value::U8(value0 & value1),
                        (Value::U16(value0), Value::U16(value1)) => Value::U16(value0 & value1),
                        (Value::U32(value0), Value::U32(value1)) => Value::U32(value0 & value1),
                        _ => unreachable!(),
                    };

                    self.machine.flags_set_sign_from_val(temp);
                    self.machine.flags_set_zero_from_val(temp);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine.flags_set_carry(false);
                    self.machine.flags_set_overflow(false);
                    // adjust flag is undefined
                }

                iced_x86::Mnemonic::Xchg => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);
                    self.machine.store_in_operand(&instruction, 0, value1);
                    self.machine.store_in_operand(&instruction, 1, value0);
                }

                iced_x86::Mnemonic::Xor => {
                    let value0 = self.machine.fetch_operand_value(&instruction, 0);
                    let value1 = self.machine.fetch_operand_value(&instruction, 1);

                    let temp = match (value0, value1) {
                        (Value::U8(value0), Value::U8(value1)) => Value::U8(value0 ^ value1),
                        (Value::U16(value0), Value::U16(value1)) => Value::U16(value0 ^ value1),
                        (Value::U32(value0), Value::U32(value1)) => Value::U32(value0 ^ value1),
                        _ => unreachable!(),
                    };

                    self.machine.store_in_operand(&instruction, 0, temp);

                    self.machine.flags_set_sign_from_val(temp);
                    self.machine.flags_set_zero_from_val(temp);
                    self.machine.flags_set_parity_from_val(temp);
                    self.machine.flags_set_carry(false);
                    self.machine.flags_set_overflow(false);
                    // adjust flag is undefined
                }

                iced_x86::Mnemonic::INVALID => return Err(Error::InvalidInstruction),
                opcode => {
                    log::error!("Unsupported instruction: {:?}", opcode);
                    return Err(Error::InvalidInstruction);
                }
            }
        }
    }
}

pub struct Machine {
    regs: Registers,
    local_memory: Vec<u8>,
}

impl Machine {
    fn read_memory(&self, addr: u32, out: &mut [u8]) {
        let out_len = u32::try_from(out.len()).unwrap();
        assert!(addr + out_len <= 0x100000);

        out.copy_from_slice(
            &self.local_memory
                [usize::try_from(addr).unwrap()..usize::try_from(addr + out_len).unwrap()],
        );
        // TODO: asyncify?
        /*redshirt_syscalls::block_on(async move {
            unsafe {
                redshirt_hardware_interface::read_to(u64::from(addr), out).await;
            }
        });*/
    }

    fn write_memory(&mut self, addr: u32, data: &[u8]) {
        let data_len = u32::try_from(data.len()).unwrap();
        assert!(addr + data_len <= 0x100000);

        // TODO: detect if we overwrite the program and reload the decoder
        // TODO: the VBE docs say that only I/O port operations are used
        self.local_memory
            [usize::try_from(addr).unwrap()..usize::try_from(addr + data_len).unwrap()]
            .copy_from_slice(data);
        /*unsafe {
            redshirt_hardware_interface::write(u64::from(addr), data);
        }*/
    }

    fn apply_rel_jump(&mut self, instruction: &iced_x86::Instruction) {
        // TODO: check segment bounds
        // TODO: this function's usefulness is debatable; it exists because I didn't realize that near_branch16() automatically calculated the target
        self.regs.eip = u32::from(instruction.near_branch16());
    }

    fn int_opcode(&mut self, vector: u8) {
        self.stack_push(&self.regs.flags.to_le_bytes());
        self.stack_push(&self.regs.cs.to_le_bytes());
        self.stack_push(
            &u16::try_from(self.regs.eip & 0xffff)
                .unwrap()
                .to_le_bytes(),
        );

        let vector = u32::from(vector);

        let mut seg = [0; 2];
        let mut ptr = [0; 2];
        self.read_memory((vector * 4) + 2, &mut seg);
        self.read_memory(vector * 4, &mut ptr);

        self.regs.cs = u16::from_le_bytes(seg);
        self.regs.eip = u32::from(u16::from_le_bytes(ptr));
    }

    /// Pushes data on the stack.
    fn stack_push(&mut self, data: &[u8]) {
        // TODO: don't panic; handle overflows, but also respect segment
        self.regs.esp = self
            .regs
            .esp
            .checked_sub(u32::try_from(data.len()).unwrap())
            .unwrap();
        let addr = (u32::from(self.regs.ss) << 4) + self.regs.esp;
        self.write_memory(addr, data);
    }

    /// Pushes a value on the stack.
    fn stack_push_value(&mut self, value: Value) {
        match value {
            Value::U8(v) => {
                let data = v.to_le_bytes();
                self.stack_push(&data);
            }
            Value::U16(v) => {
                let data = v.to_le_bytes();
                self.stack_push(&data);
            }
            Value::U32(v) => {
                let data = v.to_le_bytes();
                self.stack_push(&data);
            }
        }
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

    fn stack_pop_u8(&mut self) -> u8 {
        let mut out = [0; 1];
        self.stack_pop(&mut out);
        u8::from_le_bytes(out)
    }

    fn stack_pop_u16(&mut self) -> u16 {
        let mut out = [0; 2];
        self.stack_pop(&mut out);
        u16::from_le_bytes(out)
    }

    fn stack_pop_u32(&mut self) -> u32 {
        let mut out = [0; 4];
        self.stack_pop(&mut out);
        u32::from_le_bytes(out)
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

    fn flags_is_parity(&self) -> bool {
        (self.regs.flags & 1 << 2) != 0
    }

    fn flags_set_parity(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 2;
        } else {
            self.regs.flags &= !(1 << 2);
        }
    }

    fn flags_set_parity_from_val(&mut self, val: Value) {
        self.flags_set_parity(match val {
            Value::U8(val) => (val.count_ones() % 2) == 0,
            Value::U16(val) => ((val & 0xff).count_ones() % 2) == 0,
            Value::U32(val) => ((val & 0xff).count_ones() % 2) == 0,
        });
    }

    fn flags_is_zero(&self) -> bool {
        (self.regs.flags & 1 << 6) != 0
    }

    fn flags_set_zero_from_val(&mut self, val: Value) {
        self.flags_set_zero(val.is_zero())
    }

    fn flags_set_zero(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 6;
        } else {
            self.regs.flags &= !(1 << 6);
        }
    }

    fn flags_is_adjust(&self) -> bool {
        (self.regs.flags & 1 << 4) != 0
    }

    fn flags_set_adjust(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 4;
        } else {
            self.regs.flags &= !(1 << 4);
        }
    }

    fn flags_is_sign(&self) -> bool {
        (self.regs.flags & 1 << 7) != 0
    }

    fn flags_set_sign_from_val(&mut self, val: Value) {
        self.flags_set_sign(val.left_most_bit())
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

    fn flags_is_overflow(&self) -> bool {
        (self.regs.flags & 1 << 11) != 0
    }

    fn flags_set_overflow(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 11;
        } else {
            self.regs.flags &= !(1 << 11);
        }
    }

    fn dec_cx(&mut self) {
        let cx = u16::try_from(self.regs.ecx & 0xffff).unwrap();
        let new_cx = cx.wrapping_sub(1);
        self.regs.ecx &= 0xffff0000;
        self.regs.ecx |= u32::from(new_cx);
    }

    fn dec_si(&mut self) {
        let si = u16::try_from(self.regs.esi & 0xffff).unwrap();
        let new_si = si.wrapping_sub(1);
        self.regs.esi &= 0xffff0000;
        self.regs.esi |= u32::from(new_si);
    }

    fn inc_si(&mut self) {
        let si = u16::try_from(self.regs.esi & 0xffff).unwrap();
        let new_si = si.wrapping_add(1);
        self.regs.esi &= 0xffff0000;
        self.regs.esi |= u32::from(new_si);
    }

    fn sub_di(&mut self, n: u16) {
        let di = u16::try_from(self.regs.edi & 0xffff).unwrap();
        let new_di = di.wrapping_sub(n);
        self.regs.edi &= 0xffff0000;
        self.regs.edi |= u32::from(new_di);
    }

    fn add_di(&mut self, n: u16) {
        let di = u16::try_from(self.regs.edi & 0xffff).unwrap();
        let new_di = di.wrapping_add(n);
        self.regs.edi &= 0xffff0000;
        self.regs.edi |= u32::from(new_di);
    }

    /// Assumes that operand `op_n` of `instruction` is of type `Memory`, and loads the pointer
    /// value without the segment.
    fn memory_operand_pointer(&self, instruction: &iced_x86::Instruction, op_n: u32) -> u16 {
        assert!(matches!(
            instruction.op_kind(op_n),
            iced_x86::OpKind::Memory
        ));

        let base = match instruction.memory_base() {
            iced_x86::Register::None => 0,
            reg => match self.register(reg) {
                Value::U8(v) => u16::from(v),
                Value::U16(v) => v,
                Value::U32(v) => u16::try_from(v).unwrap(), // TODO: is this correct?
            },
        };

        let index = match instruction.memory_index() {
            iced_x86::Register::None => 0,
            reg => match self.register(reg) {
                Value::U8(v) => u16::from(v),
                Value::U16(v) => v,
                Value::U32(v) => u16::try_from(v).unwrap(), // TODO: is this correct?
            },
        };

        let index_scale = u16::try_from(instruction.memory_index_scale()).unwrap();

        let base_and_index = base.wrapping_add(index.wrapping_mul(index_scale));
        let disp = u16::try_from(instruction.memory_displacement() & 0xffff).unwrap();
        base_and_index.wrapping_add(disp)
    }

    fn fetch_operand_value(&self, instruction: &iced_x86::Instruction, op_n: u32) -> Value {
        let (segment, pointer) = match instruction.op_kind(op_n) {
            iced_x86::OpKind::Register => return self.register(instruction.op_register(op_n)),
            iced_x86::OpKind::Immediate8 => return Value::U8(instruction.immediate8()),
            iced_x86::OpKind::Immediate16 => return Value::U16(instruction.immediate16()),
            iced_x86::OpKind::Immediate32 => return Value::U32(instruction.immediate32()),
            iced_x86::OpKind::Immediate8to16 => unsafe {
                return Value::U16(mem::transmute(instruction.immediate8to16()));
                // TODO: the objective is to cast without changing any bit
            },
            iced_x86::OpKind::Immediate8to32 => unsafe {
                return Value::U32(mem::transmute(instruction.immediate8to32()));
                // TODO: the objective is to cast without changing any bit
            },
            iced_x86::OpKind::MemorySegSI => {
                let segment = u16::try_from(self.register(instruction.memory_segment())).unwrap();
                let pointer = u16::try_from(self.regs.esi & 0xffff).unwrap();
                (segment, pointer)
            }
            iced_x86::OpKind::MemoryESDI => {
                let segment = u16::try_from(self.regs.es).unwrap();
                let pointer = u16::try_from(self.regs.edi & 0xffff).unwrap();
                (segment, pointer)
            }
            iced_x86::OpKind::Memory => {
                let segment = u16::try_from(self.register(instruction.memory_segment())).unwrap();
                let pointer = self.memory_operand_pointer(instruction, op_n);
                (segment, pointer)
            }
            ty => unimplemented!("{:?}", ty),
        };

        // TODO: the memory reads are wrong; should explicitely pass segment and pointer
        let mem_address = (u32::from(segment) << 4) + u32::from(pointer);

        match instruction.memory_size().size() {
            1 => {
                let mut out = [0; 1];
                self.read_memory(mem_address, &mut out);
                Value::U8(u8::from_le_bytes(out))
            }
            2 => {
                let mut out = [0; 2];
                self.read_memory(mem_address, &mut out);
                Value::U16(u16::from_le_bytes(out))
            }
            4 => {
                let mut out = [0; 4];
                self.read_memory(mem_address, &mut out);
                Value::U32(u32::from_le_bytes(out))
            }
            _ => unreachable!(),
        }
    }

    fn register(&self, register: iced_x86::Register) -> Value {
        match register {
            iced_x86::Register::AL => Value::U8(u8::try_from(self.regs.eax & 0xff).unwrap()),
            iced_x86::Register::CL => Value::U8(u8::try_from(self.regs.ecx & 0xff).unwrap()),
            iced_x86::Register::DL => Value::U8(u8::try_from(self.regs.edx & 0xff).unwrap()),
            iced_x86::Register::BL => Value::U8(u8::try_from(self.regs.ebx & 0xff).unwrap()),
            iced_x86::Register::AH => Value::U8(u8::try_from((self.regs.eax >> 8) & 0xff).unwrap()),
            iced_x86::Register::CH => Value::U8(u8::try_from((self.regs.ecx >> 8) & 0xff).unwrap()),
            iced_x86::Register::DH => Value::U8(u8::try_from((self.regs.edx >> 8) & 0xff).unwrap()),
            iced_x86::Register::BH => Value::U8(u8::try_from((self.regs.ebx >> 8) & 0xff).unwrap()),
            iced_x86::Register::AX => Value::U16(u16::try_from(self.regs.eax & 0xffff).unwrap()),
            iced_x86::Register::CX => Value::U16(u16::try_from(self.regs.ecx & 0xffff).unwrap()),
            iced_x86::Register::DX => Value::U16(u16::try_from(self.regs.edx & 0xffff).unwrap()),
            iced_x86::Register::BX => Value::U16(u16::try_from(self.regs.ebx & 0xffff).unwrap()),
            iced_x86::Register::SP => Value::U16(u16::try_from(self.regs.esp & 0xffff).unwrap()),
            iced_x86::Register::BP => Value::U16(u16::try_from(self.regs.ebp & 0xffff).unwrap()),
            iced_x86::Register::SI => Value::U16(u16::try_from(self.regs.esi & 0xffff).unwrap()),
            iced_x86::Register::DI => Value::U16(u16::try_from(self.regs.edi & 0xffff).unwrap()),
            iced_x86::Register::EAX => Value::U32(self.regs.eax),
            iced_x86::Register::ECX => Value::U32(self.regs.ecx),
            iced_x86::Register::EDX => Value::U32(self.regs.edx),
            iced_x86::Register::EBX => Value::U32(self.regs.ebx),
            iced_x86::Register::ESP => Value::U32(self.regs.esp),
            iced_x86::Register::EBP => Value::U32(self.regs.ebp),
            iced_x86::Register::ESI => Value::U32(self.regs.esi),
            iced_x86::Register::EDI => Value::U32(self.regs.edi),
            iced_x86::Register::ES => Value::U16(self.regs.es),
            iced_x86::Register::CS => Value::U16(self.regs.cs),
            iced_x86::Register::SS => Value::U16(self.regs.ss),
            iced_x86::Register::DS => Value::U16(self.regs.ds),
            iced_x86::Register::FS => Value::U16(self.regs.fs),
            iced_x86::Register::GS => Value::U16(self.regs.gs),
            reg => unimplemented!("{:?}", reg),
        }
    }

    fn store_in_operand(&mut self, instruction: &iced_x86::Instruction, op_n: u32, val: Value) {
        let (segment, pointer) = match instruction.op_kind(op_n) {
            iced_x86::OpKind::Register => {
                return self.store_in_register(instruction.op_register(op_n), val);
            }
            iced_x86::OpKind::MemoryESDI => {
                (self.regs.es, u16::try_from(self.regs.edi & 0xffff).unwrap())
            }
            iced_x86::OpKind::Memory => {
                let segment = u16::try_from(self.register(instruction.memory_segment())).unwrap();
                let pointer = self.memory_operand_pointer(instruction, op_n);
                (segment, pointer)
            }
            ty => unimplemented!("{:?}", ty),
        };

        // TODO: the memory writes are wrong; should explicitely pass segment and pointer
        let mem_address = (u32::from(segment) << 4) + u32::from(pointer);

        match val {
            Value::U8(val) => {
                self.write_memory(mem_address, &val.to_le_bytes());
            }
            Value::U16(val) => {
                self.write_memory(mem_address, &val.to_le_bytes());
            }
            Value::U32(val) => {
                self.write_memory(mem_address, &val.to_le_bytes());
            }
        }
    }

    fn store_in_register(&mut self, register: iced_x86::Register, val: Value) {
        match (register, val) {
            (iced_x86::Register::AL, Value::U8(val)) => {
                self.regs.eax &= 0xffffff00;
                self.regs.eax |= u32::from(val);
            }
            (iced_x86::Register::CL, Value::U8(val)) => {
                self.regs.ecx &= 0xffffff00;
                self.regs.ecx |= u32::from(val);
            }
            (iced_x86::Register::DL, Value::U8(val)) => {
                self.regs.edx &= 0xffffff00;
                self.regs.edx |= u32::from(val);
            }
            (iced_x86::Register::BL, Value::U8(val)) => {
                self.regs.ebx &= 0xffffff00;
                self.regs.ebx |= u32::from(val);
            }
            (iced_x86::Register::AH, Value::U8(val)) => {
                self.regs.eax &= 0xffff00ff;
                self.regs.eax |= u32::from(val) << 8;
            }
            (iced_x86::Register::CH, Value::U8(val)) => {
                self.regs.ecx &= 0xffff00ff;
                self.regs.ecx |= u32::from(val) << 8;
            }
            (iced_x86::Register::DH, Value::U8(val)) => {
                self.regs.edx &= 0xffff00ff;
                self.regs.edx |= u32::from(val) << 8;
            }
            (iced_x86::Register::BH, Value::U8(val)) => {
                self.regs.ebx &= 0xffff00ff;
                self.regs.ebx |= u32::from(val) << 8;
            }
            (iced_x86::Register::AX, Value::U16(val)) => {
                self.regs.eax &= 0xffff0000;
                self.regs.eax |= u32::from(val);
            }
            (iced_x86::Register::CX, Value::U16(val)) => {
                self.regs.ecx &= 0xffff0000;
                self.regs.ecx |= u32::from(val);
            }
            (iced_x86::Register::DX, Value::U16(val)) => {
                self.regs.edx &= 0xffff0000;
                self.regs.edx |= u32::from(val);
            }
            (iced_x86::Register::BX, Value::U16(val)) => {
                self.regs.ebx &= 0xffff0000;
                self.regs.ebx |= u32::from(val);
            }
            (iced_x86::Register::SP, Value::U16(val)) => {
                self.regs.esp &= 0xffff0000;
                self.regs.esp |= u32::from(val);
            }
            (iced_x86::Register::BP, Value::U16(val)) => {
                self.regs.ebp &= 0xffff0000;
                self.regs.ebp |= u32::from(val);
            }
            (iced_x86::Register::SI, Value::U16(val)) => {
                self.regs.esi &= 0xffff0000;
                self.regs.esi |= u32::from(val);
            }
            (iced_x86::Register::DI, Value::U16(val)) => {
                self.regs.edi &= 0xffff0000;
                self.regs.edi |= u32::from(val);
            }
            (iced_x86::Register::EAX, Value::U32(val)) => {
                self.regs.eax = val;
            }
            (iced_x86::Register::ECX, Value::U32(val)) => {
                self.regs.ecx = val;
            }
            (iced_x86::Register::EDX, Value::U32(val)) => {
                self.regs.edx = val;
            }
            (iced_x86::Register::EBX, Value::U32(val)) => {
                self.regs.ebx = val;
            }
            (iced_x86::Register::ESP, Value::U32(val)) => {
                self.regs.esp = val;
            }
            (iced_x86::Register::EBP, Value::U32(val)) => {
                self.regs.ebp = val;
            }
            (iced_x86::Register::ESI, Value::U32(val)) => {
                self.regs.esi = val;
            }
            (iced_x86::Register::EDI, Value::U32(val)) => {
                self.regs.edi = val;
            }
            (iced_x86::Register::ES, Value::U16(val)) => {
                self.regs.es = val;
            }
            (iced_x86::Register::CS, Value::U16(_)) => {
                // Forbidden.
                panic!()
            }
            (iced_x86::Register::SS, Value::U16(val)) => {
                self.regs.ss = val;
            }
            (iced_x86::Register::DS, Value::U16(val)) => {
                self.regs.ds = val;
            }
            (iced_x86::Register::FS, Value::U16(val)) => {
                self.regs.fs = val;
            }
            (iced_x86::Register::GS, Value::U16(val)) => {
                self.regs.gs = val;
            }
            reg => unimplemented!("{:?}", reg),
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum Value {
    U8(u8),
    U16(u16),
    U32(u32),
}

impl Value {
    fn size(&self) -> u8 {
        match *self {
            Value::U8(_) => 1,
            Value::U16(_) => 2,
            Value::U32(_) => 4,
        }
    }

    fn dec(&self) -> Value {
        match *self {
            Value::U8(val) => Value::U8(val.wrapping_sub(1)),
            Value::U16(val) => Value::U16(val.wrapping_sub(1)),
            Value::U32(val) => Value::U32(val.wrapping_sub(1)),
        }
    }

    fn left_most_bit(&self) -> bool {
        match *self {
            Value::U8(val) => (val & 0x80) != 0,
            Value::U16(val) => (val & 0x8000) != 0,
            Value::U32(val) => (val & 0x80000000) != 0,
        }
    }

    fn extend_to_u32(&self) -> u32 {
        match *self {
            Value::U8(val) => u32::from(val),
            Value::U16(val) => u32::from(val),
            Value::U32(val) => val,
        }
    }

    fn is_zero(&self) -> bool {
        match *self {
            Value::U8(val) => val == 0,
            Value::U16(val) => val == 0,
            Value::U32(val) => val == 0,
        }
    }
}

impl TryFrom<Value> for u8 {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, ()> {
        if let Value::U8(v) = v {
            Ok(v)
        } else {
            Err(())
        }
    }
}

impl TryFrom<Value> for u16 {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, ()> {
        if let Value::U16(v) = v {
            Ok(v)
        } else {
            Err(())
        }
    }
}

impl TryFrom<Value> for u32 {
    type Error = ();
    fn try_from(v: Value) -> Result<Self, ()> {
        if let Value::U32(v) = v {
            Ok(v)
        } else {
            Err(())
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
