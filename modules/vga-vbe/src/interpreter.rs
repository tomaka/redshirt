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

use core::{convert::TryFrom, fmt};

mod tests;

/// Intel 80386 real mode interpreter.
pub struct Interpreter {
    /// State of all the registers of the CPU, including CS and EIP.
    regs: Registers,
    /// Cache of the first megabyte of memory.
    memory_cache: Vec<u8>,
    /// If true, perform I/O ports operations on the actual machine. Otherwise, reading a port
    /// returns 0 and writing a port is a no-op.
    enable_io_operations: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct Registers {
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

#[derive(Debug)]
pub enum Error {
    InvalidInstruction,

    /// Code has called the `hlt` instruction, which cannot do anything.
    InterruptNotSupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::InvalidInstruction => write!(f, "Invalid instruction"),
            Error::InterruptNotSupported => write!(f, "HLT has been called"),
        }
    }
}

impl Interpreter {
    pub async fn from_real_machine() -> Self {
        let first_mb = unsafe { redshirt_hardware_interface::read(0x0, 0x100000).await };
        // Small sanity check.
        assert!(first_mb.iter().any(|b| *b != 0));
        Self::from_memory(first_mb).await
    }

    pub async fn from_memory(first_mb: Vec<u8>) -> Self {
        assert_eq!(first_mb.len(), 0x100000);

        Interpreter {
            memory_cache: first_mb,
            regs: Registers {
                eax: 0,
                ecx: 0,
                edx: 0,
                ebx: 0,
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
            enable_io_operations: true,
        }
    }

    /// After this is called, I/O operations will no longer be performed on the actual machine.
    /// Reading a port now always returns 0, and writing a port becomes a no-op. Reading and
    /// writing memory is only done on the local cache.
    ///
    /// > **Note**: It is intentional that no opposite function is provided, as the memory stops
    /// >           being in sync with the actual memory, which will likely cause issues.
    #[cfg_attr(not(test), allow(unused))] // Used only in tests.
    pub fn disable_io_operations(&mut self) {
        self.enable_io_operations = false;
    }

    /// Reads bytes from the physical memory.
    ///
    /// This will read the memory of the actual machine.
    ///
    /// # Panic
    ///
    /// Panics if the memory address and size are out of range.
    ///
    pub fn read_memory(&mut self, addr: u32, out: &mut [u8]) {
        let out_len = u32::try_from(out.len()).unwrap();
        assert!(addr + out_len <= 0x100000);

        // Perform a cache refresh only if we hit the video memory.
        if self.enable_io_operations && addr >= 0xa0000 && addr < 0xc0000 {
            // TODO: asyncify?
            redshirt_syscalls::block_on(async {
                unsafe {
                    redshirt_hardware_interface::read_to(u64::from(addr), out).await;
                }
            });
            self.memory_cache
                [usize::try_from(addr).unwrap()..usize::try_from(addr + out_len).unwrap()]
                .copy_from_slice(&out);
        } else {
            out.copy_from_slice(
                &self.memory_cache
                    [usize::try_from(addr).unwrap()..usize::try_from(addr + out_len).unwrap()],
            );
        }
    }

    pub fn read_memory_nul_terminated_str(&mut self, mut addr: u32) -> String {
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

    pub fn read_memory_u8(&mut self, addr: u32) -> u8 {
        let mut out = [0; 1];
        self.read_memory(addr, &mut out);
        u8::from_le_bytes(out)
    }

    pub fn read_memory_u16(&mut self, addr: u32) -> u16 {
        let mut out = [0; 2];
        self.read_memory(addr, &mut out);
        u16::from_le_bytes(out)
    }

    pub fn write_memory(&mut self, addr: u32, data: &[u8]) {
        let data_len = u32::try_from(data.len()).unwrap();
        assert!(addr + data_len <= 0x100000);

        self.memory_cache
            [usize::try_from(addr).unwrap()..usize::try_from(addr + data_len).unwrap()]
            .copy_from_slice(data);

        // We only perform the actual write to memory if we hit the video memory.
        if self.enable_io_operations && addr >= 0xa0000 && addr < 0xc0000 {
            unsafe {
                redshirt_hardware_interface::write(u64::from(addr), data);
            }
        }
    }

    pub fn ax(&mut self) -> u16 {
        u16::try_from(self.regs.eax & 0xffff).unwrap()
    }

    pub fn set_ax(&mut self, value: u16) {
        self.regs.eax &= 0xffff0000;
        self.regs.eax |= u32::from(value);
    }

    pub fn set_bx(&mut self, value: u16) {
        self.regs.ebx &= 0xffff0000;
        self.regs.ebx |= u32::from(value);
    }

    pub fn cx(&mut self) -> u16 {
        u16::try_from(self.regs.ecx & 0xffff).unwrap()
    }

    pub fn set_cx(&mut self, value: u16) {
        self.regs.ecx &= 0xffff0000;
        self.regs.ecx |= u32::from(value);
    }

    pub fn dx(&mut self) -> u16 {
        u16::try_from(self.regs.edx & 0xffff).unwrap()
    }

    pub fn set_es_di(&mut self, es: u16, di: u16) {
        self.regs.es = es;
        self.regs.edi &= 0xffff0000;
        self.regs.edi |= u32::from(di);
    }

    /// Executes the `int 0x10` instruction on the machine, and run until the corresponding `iret`
    /// instruction is executed.
    pub fn int10h(&mut self) -> Result<(), Error> {
        self.run_int_opcode(0x10);
        self.run_until_iret()
    }

    /// Runs the machine until the `iret` instruction is executed.
    ///
    /// Nested interrupts are accounted for. If an `int` opcode is executed, then the next `iret`
    /// will not cause this function to finish.
    fn run_until_iret(&mut self) -> Result<(), Error> {
        // Counts the number of nested interrupts. Incremented when `int` is called, decremented
        // when `iret` is called.
        let mut nested_ints: u32 = 0;

        loop {
            // Decode instruction and update the IP register.
            let instruction = {
                let rip = (u64::from(self.regs.cs) << 4) + u64::from(self.regs.eip);
                assert!(usize::try_from(rip).unwrap() < self.memory_cache.len());

                // We recreate a `Decoder` at each iteration because we need to be able to modify
                // the memory during the processing of the instruction. While it is unlikely to
                // actually happen, we do need to support self-modifying programs.
                let mut decoder =
                    iced_x86::Decoder::new(16, &self.memory_cache, iced_x86::DecoderOptions::NONE);
                decoder.set_position(usize::try_from(rip).unwrap());
                decoder.set_ip(rip);

                let instruction = decoder.decode();
                assert!(!instruction.has_xrelease_prefix());
                self.regs.eip = {
                    let ip = self.ip();
                    let new_ip = ip.wrapping_add(u16::try_from(instruction.len()).unwrap());
                    u32::from(new_ip)
                };

                instruction
            };

            self.run_one(&instruction)?;

            match instruction.mnemonic() {
                iced_x86::Mnemonic::Iret if nested_ints == 0 => break Ok(()),
                iced_x86::Mnemonic::Iret => nested_ints -= 1,
                iced_x86::Mnemonic::Int => nested_ints += 1,
                _ => {}
            }
        }
    }

    /// Apply the given instruction on the current state of the machine.
    fn run_one(&mut self, instruction: &iced_x86::Instruction) -> Result<(), Error> {
        if !instruction.has_rep_prefix()
            && !instruction.has_repe_prefix()
            && !instruction.has_repne_prefix()
        {
            return self.run_one_no_rep(instruction);
        }

        // At this point we know we have a REP/REPE/REPNE prefix.

        // Determining whether to use CX or ECX is surprinsingly impossible with the iced-x86
        // library.
        let use_ecx = (0..instruction.op_count()).any(|op_n| match instruction.op_kind(op_n) {
            iced_x86::OpKind::MemorySegEDI => true,
            iced_x86::OpKind::MemorySegESI => true,
            iced_x86::OpKind::MemoryESEDI => true,
            _ => false,
        });

        loop {
            if (use_ecx && self.regs.ecx == 0) || (!use_ecx && self.cx() == 0) {
                break;
            }

            self.run_one_no_rep(instruction)?;

            if use_ecx {
                self.regs.ecx = self.regs.ecx.wrapping_sub(1);
            } else {
                self.dec_cx();
            }

            if instruction.has_repne_prefix() && !self.flags_is_zero() {
                break;
            }

            // Unfortunately, the REP and REPE prefixes are the same, and which one is which
            // depends on the mnemonic. This `if` block checks whether we have a REPE prefix, as
            // opposed to just REP.
            if let iced_x86::Mnemonic::Cmpsb
            | iced_x86::Mnemonic::Cmpsd
            | iced_x86::Mnemonic::Cmpsq
            | iced_x86::Mnemonic::Cmpss
            | iced_x86::Mnemonic::Cmpsw
            | iced_x86::Mnemonic::Scasb
            | iced_x86::Mnemonic::Scasd
            | iced_x86::Mnemonic::Scasq
            | iced_x86::Mnemonic::Scasw = instruction.mnemonic()
            {
                if self.flags_is_zero() {
                    break;
                }
            }
        }

        Ok(())
    }

    /// Same as [`Interpreter::run_one`] but doesn't care about any `REP`/`REPE`/`REPNE` prefix.
    fn run_one_no_rep(&mut self, instruction: &iced_x86::Instruction) -> Result<(), Error> {
        if instruction.is_protected_mode() {
            return Err(Error::InvalidInstruction);
        }

        // List here: https://en.wikipedia.org/wiki/X86_instruction_listings#Original_8086/8088_instructions
        // The objective is to implement up to and including the x386.
        // TODO: finish implementing all instructions marked as `todo!()`
        match instruction.mnemonic() {
            iced_x86::Mnemonic::Aaa => todo!(),
            iced_x86::Mnemonic::Aad => todo!(),
            iced_x86::Mnemonic::Aam => todo!(),
            iced_x86::Mnemonic::Aas => todo!(),

            iced_x86::Mnemonic::Adc => todo!(),

            iced_x86::Mnemonic::Add => {
                let value0 = self.fetch_operand_value(&instruction, 0);
                let value1 = self.fetch_operand_value(&instruction, 1);

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

                self.store_in_operand(&instruction, 0, temp);

                self.flags_set_sign_from_val(temp);
                self.flags_set_zero_from_val(temp);
                self.flags_set_parity_from_val(temp);
                self.flags_set_carry(overflow);
                self.flags_set_overflow(overflow != temp.most_significant_bit());
                // TODO: the adjust flag
            }

            iced_x86::Mnemonic::And | iced_x86::Mnemonic::Or | iced_x86::Mnemonic::Xor => {
                let value0 = self.fetch_operand_value(&instruction, 0);
                let value1 = self.fetch_operand_value(&instruction, 1);

                let temp = match (value0, value1, instruction.mnemonic()) {
                    (Value::U8(value0), Value::U8(value1), iced_x86::Mnemonic::And) => {
                        Value::U8(value0 & value1)
                    }
                    (Value::U16(value0), Value::U16(value1), iced_x86::Mnemonic::And) => {
                        Value::U16(value0 & value1)
                    }
                    (Value::U32(value0), Value::U32(value1), iced_x86::Mnemonic::And) => {
                        Value::U32(value0 & value1)
                    }
                    (Value::U8(value0), Value::U8(value1), iced_x86::Mnemonic::Or) => {
                        Value::U8(value0 | value1)
                    }
                    (Value::U16(value0), Value::U16(value1), iced_x86::Mnemonic::Or) => {
                        Value::U16(value0 | value1)
                    }
                    (Value::U32(value0), Value::U32(value1), iced_x86::Mnemonic::Or) => {
                        Value::U32(value0 | value1)
                    }
                    (Value::U8(value0), Value::U8(value1), iced_x86::Mnemonic::Xor) => {
                        Value::U8(value0 ^ value1)
                    }
                    (Value::U16(value0), Value::U16(value1), iced_x86::Mnemonic::Xor) => {
                        Value::U16(value0 ^ value1)
                    }
                    (Value::U32(value0), Value::U32(value1), iced_x86::Mnemonic::Xor) => {
                        Value::U32(value0 ^ value1)
                    }
                    _ => unreachable!(),
                };

                self.store_in_operand(&instruction, 0, temp);

                self.flags_set_sign_from_val(temp);
                self.flags_set_zero_from_val(temp);
                self.flags_set_parity_from_val(temp);
                self.flags_set_carry(false);
                self.flags_set_overflow(false);
                // adjust flag is undefined
            }

            iced_x86::Mnemonic::Bound => todo!(),

            iced_x86::Mnemonic::Bsf => todo!(),
            iced_x86::Mnemonic::Bsr => todo!(),

            iced_x86::Mnemonic::Bt => {
                let value0 = self.fetch_operand_value(&instruction, 0);
                let value1 = self.fetch_operand_value(&instruction, 1);

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

                self.flags_set_carry(bit);
            }

            iced_x86::Mnemonic::Btc => todo!(),
            iced_x86::Mnemonic::Btr => todo!(),
            iced_x86::Mnemonic::Bts => todo!(),

            iced_x86::Mnemonic::Call => {
                match instruction.code() {
                    iced_x86::Code::Call_ptr1616 | iced_x86::Code::Call_m1616 => {
                        let ip = self.ip();
                        self.stack_push_value(Value::U16(self.regs.cs));
                        self.stack_push_value(Value::U16(ip));
                    }
                    iced_x86::Code::Call_rel16 | iced_x86::Code::Call_rm16 => {
                        let ip = self.ip();
                        self.stack_push_value(Value::U16(ip));
                    }
                    _ => unreachable!(),
                }

                self.apply_jump(&instruction);
            }

            iced_x86::Mnemonic::Cbw => {
                let al = u8::try_from(self.register(iced_x86::Register::AL)).unwrap();
                let msb = (al & 0x80) != 0;
                if msb {
                    self.set_ax(0xff00 | u16::from(al));
                } else {
                    self.set_ax(u16::from(al));
                }
            }

            iced_x86::Mnemonic::Cwde => {
                let ax = u16::try_from(self.register(iced_x86::Register::AX)).unwrap();
                let msb = (ax & 0x8000) != 0;
                if msb {
                    self.regs.eax = 0xffff0000 | u32::from(ax);
                } else {
                    self.regs.eax = u32::from(ax);
                }
            }

            iced_x86::Mnemonic::Cwd => {
                if self.register(iced_x86::Register::AX).most_significant_bit() {
                    self.store_in_register(iced_x86::Register::DX, Value::U16(0xffff))
                } else {
                    self.store_in_register(iced_x86::Register::DX, Value::U16(0x0000))
                }
            }

            iced_x86::Mnemonic::Cdq => {
                if self
                    .register(iced_x86::Register::EAX)
                    .most_significant_bit()
                {
                    self.store_in_register(iced_x86::Register::EDX, Value::U32(0xffffffff))
                } else {
                    self.store_in_register(iced_x86::Register::EDX, Value::U32(0x00000000))
                }
            }

            iced_x86::Mnemonic::Clc => self.flags_set_carry(false),
            iced_x86::Mnemonic::Cld => self.flags_set_direction(false),
            iced_x86::Mnemonic::Cli => self.flags_set_interrupt(false),
            iced_x86::Mnemonic::Cmc => self.flags_set_carry(!self.flags_is_carry()),

            iced_x86::Mnemonic::Cmp => {
                let value0 = self.fetch_operand_value(&instruction, 0);
                let value1 = self.fetch_operand_value(&instruction, 1);

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

                self.flags_set_sign_from_val(temp);
                self.flags_set_zero_from_val(temp);
                self.flags_set_parity_from_val(temp);
                self.flags_set_carry(overflow);
                self.flags_set_overflow(overflow != temp.most_significant_bit());
                // TODO: the adjust flag
            }

            iced_x86::Mnemonic::Cmpsb | iced_x86::Mnemonic::Cmpsw | iced_x86::Mnemonic::Cmpsd => {
                todo!()
            }

            iced_x86::Mnemonic::Daa => todo!(),
            iced_x86::Mnemonic::Das => todo!(),

            iced_x86::Mnemonic::Dec => {
                let value = self.fetch_operand_value(&instruction, 0);
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
                };

                self.store_in_operand(&instruction, 0, temp);

                self.flags_set_sign_from_val(temp);
                self.flags_set_zero_from_val(temp);
                self.flags_set_parity_from_val(temp);
                self.flags_set_overflow(overflow != temp.most_significant_bit());
                // TODO: the adjust flag
                // Carry flag is not affected.
            }

            iced_x86::Mnemonic::Div => {
                // TODO: no check for division by zero
                match self.fetch_operand_value(&instruction, 0) {
                    Value::U8(divisor) => {
                        let dividend = u16::try_from(self.regs.eax & 0xffff).unwrap();
                        let divisor = u16::from(divisor);
                        let quotient = u8::try_from((dividend / divisor) & 0xff).unwrap();
                        let remainder = u8::try_from(dividend % divisor).unwrap();
                        self.store_in_register(iced_x86::Register::AL, Value::U8(quotient));
                        self.store_in_register(iced_x86::Register::AH, Value::U8(remainder));
                    }
                    Value::U16(divisor) => {
                        let dividend = u32::try_from(
                            ((self.regs.edx & 0xffff) << 16) | (self.regs.eax & 0xffff),
                        )
                        .unwrap();
                        let divisor = u32::from(divisor);
                        let quotient = u16::try_from((dividend / divisor) & 0xffff).unwrap();
                        let remainder = u16::try_from(dividend % divisor).unwrap();
                        self.store_in_register(iced_x86::Register::AX, Value::U16(quotient));
                        self.store_in_register(iced_x86::Register::DX, Value::U16(remainder));
                    }
                    Value::U32(divisor) => {
                        let dividend = (u64::from(self.regs.edx) << 32) | u64::from(self.regs.eax);
                        let divisor = u64::from(divisor);
                        let quotient = u32::try_from((dividend / divisor) & 0xffffffff).unwrap();
                        let remainder = u32::try_from(dividend % divisor).unwrap();
                        self.regs.eax = quotient;
                        self.regs.edx = remainder;
                    }
                }
            }

            iced_x86::Mnemonic::Enter => todo!(),

            iced_x86::Mnemonic::Hlt => return Err(Error::InterruptNotSupported),

            iced_x86::Mnemonic::Idiv => {
                // TODO: no check for division by zero
                match self.fetch_operand_value(&instruction, 0) {
                    Value::U8(divisor) => {
                        let dividend = i16::from_ne_bytes(self.ax().to_ne_bytes());
                        let divisor = i16::from_ne_bytes(
                            (i16::from(i8::from_ne_bytes(divisor.to_ne_bytes()))).to_ne_bytes(),
                        );
                        let quotient = u16::from_ne_bytes((dividend / divisor).to_ne_bytes());
                        let quotient = u8::try_from(quotient & 0xff).unwrap();
                        let remainder = u16::from_ne_bytes((dividend % divisor).to_ne_bytes());
                        let remainder = u8::try_from(remainder & 0xff).unwrap();
                        self.store_in_register(iced_x86::Register::AL, Value::U8(quotient));
                        self.store_in_register(iced_x86::Register::AH, Value::U8(remainder));
                    }
                    Value::U16(divisor) => {
                        let dividend = i32::from_ne_bytes(
                            ((u32::from(self.dx()) << 16) | u32::from(self.ax())).to_ne_bytes(),
                        );
                        let divisor = i32::from(i16::from_ne_bytes(divisor.to_ne_bytes()));
                        let quotient = u32::from_ne_bytes((dividend / divisor).to_ne_bytes());
                        let quotient = u16::try_from(quotient & 0xffff).unwrap();
                        let remainder = u32::from_ne_bytes((dividend % divisor).to_ne_bytes());
                        let remainder = u16::try_from(remainder & 0xffff).unwrap();
                        self.store_in_register(iced_x86::Register::AX, Value::U16(quotient));
                        self.store_in_register(iced_x86::Register::DX, Value::U16(remainder));
                    }
                    Value::U32(divisor) => {
                        let dividend = i64::from_ne_bytes(
                            ((u64::from(self.regs.edx) << 32) | u64::from(self.regs.eax))
                                .to_ne_bytes(),
                        );
                        let divisor = i64::from(i32::from_ne_bytes(divisor.to_ne_bytes()));
                        let quotient = u64::from_ne_bytes((dividend / divisor).to_ne_bytes());
                        let quotient = u32::try_from(quotient & 0xffffffff).unwrap();
                        let remainder = u64::from_ne_bytes((dividend % divisor).to_ne_bytes());
                        let remainder = u32::try_from(remainder & 0xffffffff).unwrap();
                        self.regs.eax = quotient;
                        self.regs.edx = remainder;
                    }
                }

                // Flags are undefined.
            }

            iced_x86::Mnemonic::Imul => {
                let (to_mul1, to_mul2) = if instruction.op_count() == 3 {
                    let value1 = self.fetch_operand_value(&instruction, 1);
                    let value2 = self.fetch_operand_value(&instruction, 2);
                    (value1, value2)
                } else {
                    // TODO: is that correct when op_count() == 1?
                    let value0 = self.fetch_operand_value(&instruction, 0);
                    let value1 = self.fetch_operand_value(&instruction, 1);
                    (value0, value1)
                };

                // Signed multiplication of `to_mul1` and `to_mul2`. The highest and lowest half
                // of the result in `result_hi` and `result_lo` are reinterpreted as unsigned
                // integers.
                let (result_hi, result_lo) = match (to_mul1, to_mul2) {
                    (Value::U8(to_mul1), Value::U8(to_mul2)) => {
                        let to_mul1 = i16::from(i8::from_ne_bytes(to_mul1.to_ne_bytes()));
                        let to_mul2 = i16::from(i8::from_ne_bytes(to_mul2.to_ne_bytes()));
                        let result = to_mul1.checked_mul(to_mul2).unwrap();
                        let result = u16::from_ne_bytes(result.to_ne_bytes());
                        let result_lo = u8::try_from(result & 0xff).unwrap();
                        let result_hi = u8::try_from(result >> 8).unwrap();
                        (Value::U8(result_hi), Value::U8(result_lo))
                    }
                    (Value::U16(to_mul1), Value::U16(to_mul2)) => {
                        let to_mul1 = i32::from(i16::from_ne_bytes(to_mul1.to_ne_bytes()));
                        let to_mul2 = i32::from(i16::from_ne_bytes(to_mul2.to_ne_bytes()));
                        let result = to_mul1.checked_mul(to_mul2).unwrap();
                        let result = u32::from_ne_bytes(result.to_ne_bytes());
                        let result_lo = u16::try_from(result & 0xffff).unwrap();
                        let result_hi = u16::try_from(result >> 16).unwrap();
                        (Value::U16(result_hi), Value::U16(result_lo))
                    }
                    (Value::U32(to_mul1), Value::U32(to_mul2)) => {
                        let to_mul1 = i64::from(i32::from_ne_bytes(to_mul1.to_ne_bytes()));
                        let to_mul2 = i64::from(i32::from_ne_bytes(to_mul2.to_ne_bytes()));
                        let result = to_mul1.checked_mul(to_mul2).unwrap();
                        let result = u64::from_ne_bytes(result.to_ne_bytes());
                        let result_lo = u32::try_from(result & 0xffffffff).unwrap();
                        let result_hi = u32::try_from(result >> 32).unwrap();
                        (Value::U32(result_hi), Value::U32(result_lo))
                    }
                    _ => unreachable!(),
                };

                self.store_in_operand(&instruction, 0, result_lo);

                match instruction.code() {
                    iced_x86::Code::Imul_rm8 => {
                        self.store_in_register(iced_x86::Register::AH, result_hi);
                    }
                    iced_x86::Code::Imul_rm16 => {
                        self.store_in_register(iced_x86::Register::DX, result_hi);
                    }
                    iced_x86::Code::Imul_rm32 => {
                        self.store_in_register(iced_x86::Register::EDX, result_hi);
                    }
                    _ => {}
                };

                // "The CF and OF flags are set when the signed integer value of the intermediate
                // product differs from the sign extended operand-size-truncated product, otherwise
                // the CF and OF flags are cleared." - Intel manual
                let overflow = if result_lo.most_significant_bit() {
                    !result_hi.is_max_value()
                } else {
                    !result_hi.is_zero()
                };

                self.flags_set_carry(overflow);
                self.flags_set_overflow(overflow);
                // Sign, zero, parity and adjust flags are undefined.
            }

            iced_x86::Mnemonic::In => {
                let port = match self.fetch_operand_value(&instruction, 1) {
                    Value::U8(p) => u16::from(p),
                    Value::U16(p) => p,
                    _ => unreachable!(),
                };

                let data = if self.enable_io_operations {
                    match self.fetch_operand_value(&instruction, 0) {
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
                    }
                } else {
                    match self.fetch_operand_value(&instruction, 0) {
                        Value::U8(_) => Value::U8(0),
                        Value::U16(_) => Value::U16(0),
                        Value::U32(_) => Value::U32(0),
                    }
                };

                self.store_in_operand(&instruction, 0, data);
            }

            iced_x86::Mnemonic::Inc => {
                let value = self.fetch_operand_value(&instruction, 0);
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
                };

                self.store_in_operand(&instruction, 0, temp);

                self.flags_set_sign_from_val(temp);
                self.flags_set_zero_from_val(temp);
                self.flags_set_parity_from_val(temp);
                self.flags_set_overflow(overflow != temp.most_significant_bit());
                // TODO: the adjust flag
                // Carry flag is not affected.
            }

            iced_x86::Mnemonic::Insb | iced_x86::Mnemonic::Insd | iced_x86::Mnemonic::Insw => {
                todo!()
            }

            iced_x86::Mnemonic::Int => {
                let value = self.fetch_operand_value(&instruction, 0);
                self.run_int_opcode(u8::try_from(value).unwrap());
                log::info!("Int 0x{:x}", u8::try_from(value).unwrap());
            }

            iced_x86::Mnemonic::Into => todo!(),

            iced_x86::Mnemonic::Iret => {
                let ip = self.stack_pop_u16();
                self.regs.eip = u32::from(ip);
                let cs = self.stack_pop_u16();
                self.regs.cs = cs;

                let val = self.stack_pop_u16();
                self.regs.flags &= 0b01000000000101010;
                self.regs.flags |= val & 0b0111111111010101;
            }

            iced_x86::Mnemonic::Ja
            | iced_x86::Mnemonic::Jae
            | iced_x86::Mnemonic::Jb
            | iced_x86::Mnemonic::Jbe
            | iced_x86::Mnemonic::Jcxz
            | iced_x86::Mnemonic::Je
            | iced_x86::Mnemonic::Jecxz
            | iced_x86::Mnemonic::Jg
            | iced_x86::Mnemonic::Jge
            | iced_x86::Mnemonic::Jl
            | iced_x86::Mnemonic::Jle
            | iced_x86::Mnemonic::Jne
            | iced_x86::Mnemonic::Jno
            | iced_x86::Mnemonic::Jnp
            | iced_x86::Mnemonic::Jns
            | iced_x86::Mnemonic::Jo
            | iced_x86::Mnemonic::Jp
            | iced_x86::Mnemonic::Js => {
                debug_assert!(!matches!(
                    instruction.condition_code(),
                    iced_x86::ConditionCode::None
                ));
                if self.flags_check_condition(instruction.condition_code()) {
                    self.apply_jump(&instruction);
                }
            }

            iced_x86::Mnemonic::Jmp => {
                self.apply_jump(&instruction);
            }

            iced_x86::Mnemonic::Lahf => todo!(),
            iced_x86::Mnemonic::Lar => todo!(),

            iced_x86::Mnemonic::Lea => {
                let ptr = self.memory_operand_pointer(&instruction, 1);
                self.store_in_operand(&instruction, 0, Value::U16(ptr));
            }

            iced_x86::Mnemonic::Leave => todo!(),

            iced_x86::Mnemonic::Lgs
            | iced_x86::Mnemonic::Lss
            | iced_x86::Mnemonic::Lds
            | iced_x86::Mnemonic::Les
            | iced_x86::Mnemonic::Lfs => todo!(),

            iced_x86::Mnemonic::Lodsb => {
                // TODO: review this
                let val = self.fetch_operand_value(&instruction, 1);

                let use_esi = match instruction.op_kind(1) {
                    iced_x86::OpKind::MemorySegSI => false,
                    iced_x86::OpKind::MemorySegESI => true,
                    _ => unreachable!(),
                };

                self.store_in_operand(&instruction, 0, val);

                if use_esi {
                    if self.flags_is_direction() {
                        self.regs.esi = self.regs.esi.wrapping_sub(u32::from(val.size()));
                    } else {
                        self.regs.esi = self.regs.esi.wrapping_add(u32::from(val.size()));
                    }
                } else {
                    if self.flags_is_direction() {
                        self.sub_si(u16::from(val.size()));
                    } else {
                        self.add_si(u16::from(val.size()));
                    }
                }
            }

            iced_x86::Mnemonic::Lodsw | iced_x86::Mnemonic::Lodsd => todo!(),

            iced_x86::Mnemonic::Loop | iced_x86::Mnemonic::Loope | iced_x86::Mnemonic::Loopne => {
                let use_ecx = match instruction.code() {
                    iced_x86::Code::Loop_rel8_16_CX => false,
                    iced_x86::Code::Loop_rel8_32_CX => false,
                    iced_x86::Code::Loop_rel8_16_ECX => true,
                    iced_x86::Code::Loop_rel8_32_ECX => true,
                    iced_x86::Code::Loop_rel8_64_ECX => true,
                    iced_x86::Code::Loope_rel8_16_CX => false,
                    iced_x86::Code::Loope_rel8_32_CX => false,
                    iced_x86::Code::Loope_rel8_16_ECX => true,
                    iced_x86::Code::Loope_rel8_32_ECX => true,
                    iced_x86::Code::Loope_rel8_64_ECX => true,
                    iced_x86::Code::Loopne_rel8_16_CX => false,
                    iced_x86::Code::Loopne_rel8_32_CX => false,
                    iced_x86::Code::Loopne_rel8_16_ECX => true,
                    iced_x86::Code::Loopne_rel8_32_ECX => true,
                    iced_x86::Code::Loopne_rel8_64_ECX => true,
                    _ => false,
                };

                let is_zero = if use_ecx {
                    self.regs.ecx = self.regs.ecx.wrapping_sub(1);
                    self.regs.ecx == 0
                } else {
                    let cx = self.cx();
                    let cx = cx.wrapping_sub(1);
                    self.set_cx(cx);
                    cx == 0
                };

                let could_jump = match instruction.mnemonic() {
                    iced_x86::Mnemonic::Loop => true,
                    iced_x86::Mnemonic::Loope => self.flags_is_zero(),
                    iced_x86::Mnemonic::Loopne => !self.flags_is_zero(),
                    _ => unreachable!(),
                };

                if !is_zero && could_jump {
                    self.apply_jump(&instruction);
                }
            }

            iced_x86::Mnemonic::Mov => {
                // When executing `mov reg, sreg`, the upper bits of `reg` are zeroed on modern
                // processors.
                if let iced_x86::OpKind::Register = instruction.op_kind(1) {
                    match instruction.op_register(1) {
                        iced_x86::Register::ES
                        | iced_x86::Register::CS
                        | iced_x86::Register::SS
                        | iced_x86::Register::DS
                        | iced_x86::Register::FS
                        | iced_x86::Register::GS => match self.operand_size(&instruction, 0) {
                            2 => {}
                            4 => self.store_in_operand(&instruction, 0, Value::U32(0)),
                            _ => unreachable!(),
                        },
                        _ => {}
                    }
                }

                let value = self.fetch_operand_value(&instruction, 1);
                self.store_in_operand(&instruction, 0, value);
            }

            iced_x86::Mnemonic::Movsb | iced_x86::Mnemonic::Movsw | iced_x86::Mnemonic::Movsd => {
                todo!()
            }

            iced_x86::Mnemonic::Movsx => {
                let value = self.fetch_operand_value(&instruction, 1);
                let msb = value.most_significant_bit();

                match (self.operand_size(&instruction, 0), value, msb) {
                    (2, Value::U8(v), true) => {
                        self.store_in_operand(&instruction, 0, Value::U16(0xff | u16::from(v)))
                    }
                    (2, Value::U8(v), false) => {
                        self.store_in_operand(&instruction, 0, Value::U16(u16::from(v)))
                    }
                    (4, Value::U8(v), true) => {
                        self.store_in_operand(&instruction, 0, Value::U32(0xffffff | u32::from(v)))
                    }
                    (4, Value::U8(v), false) => {
                        self.store_in_operand(&instruction, 0, Value::U32(u32::from(v)))
                    }
                    (4, Value::U16(v), true) => {
                        self.store_in_operand(&instruction, 0, Value::U32(0xffff | u32::from(v)))
                    }
                    (4, Value::U16(v), false) => {
                        self.store_in_operand(&instruction, 0, Value::U32(u32::from(v)))
                    }
                    _ => unreachable!(),
                }
            }

            iced_x86::Mnemonic::Movzx => {
                let value = self.fetch_operand_value(&instruction, 1);

                match (self.operand_size(&instruction, 0), value) {
                    (2, Value::U8(v)) => {
                        self.store_in_operand(&instruction, 0, Value::U16(u16::from(v)))
                    }
                    (4, Value::U8(v)) => {
                        self.store_in_operand(&instruction, 0, Value::U32(u32::from(v)))
                    }
                    (4, Value::U16(v)) => {
                        self.store_in_operand(&instruction, 0, Value::U32(u32::from(v)))
                    }
                    _ => unreachable!(),
                }
            }

            iced_x86::Mnemonic::Mul => {
                let value = self.fetch_operand_value(&instruction, 0);

                let (result_hi, result_lo) = match value {
                    Value::U8(to_mul1) => {
                        let to_mul1 = u16::from(to_mul1);
                        let to_mul2 =
                            u16::from(u8::try_from(self.register(iced_x86::Register::AL)).unwrap());
                        let result = to_mul1.checked_mul(to_mul2).unwrap();
                        let result_lo = u8::try_from(result & 0xff).unwrap();
                        let result_hi = u8::try_from(result >> 8).unwrap();
                        (Value::U8(result_hi), Value::U8(result_lo))
                    }
                    Value::U16(to_mul1) => {
                        let to_mul1 = u32::from(to_mul1);
                        let to_mul2 = u32::from(self.ax());
                        let result = to_mul1.checked_mul(to_mul2).unwrap();
                        let result_lo = u16::try_from(result & 0xffff).unwrap();
                        let result_hi = u16::try_from(result >> 16).unwrap();
                        (Value::U16(result_hi), Value::U16(result_lo))
                    }
                    Value::U32(to_mul1) => {
                        let to_mul1 = u64::from(to_mul1);
                        let to_mul2 = u64::from(self.regs.eax);
                        let result = to_mul1.checked_mul(to_mul2).unwrap();
                        let result_lo = u32::try_from(result & 0xffffffff).unwrap();
                        let result_hi = u32::try_from(result >> 32).unwrap();
                        (Value::U32(result_hi), Value::U32(result_lo))
                    }
                };

                self.store_in_operand(&instruction, 0, result_lo);

                match result_hi {
                    v @ Value::U8(_) => self.store_in_register(iced_x86::Register::AH, v),
                    v @ Value::U16(_) => self.store_in_register(iced_x86::Register::DX, v),
                    v @ Value::U32(_) => self.store_in_register(iced_x86::Register::EDX, v),
                }

                self.flags_set_carry(!result_hi.is_zero());
                self.flags_set_overflow(!result_hi.is_zero());
                // Sign, zero, parity and adjust flags are undefined.
            }

            iced_x86::Mnemonic::Nop => {}

            iced_x86::Mnemonic::Not => {
                let value = self.fetch_operand_value(&instruction, 0);
                let result = match value {
                    Value::U8(value) => Value::U8(!value),
                    Value::U16(value) => Value::U16(!value),
                    Value::U32(value) => Value::U32(!value),
                };
                self.store_in_operand(&instruction, 0, result);
            }

            iced_x86::Mnemonic::Out => {
                let port = match self.fetch_operand_value(&instruction, 0) {
                    Value::U8(p) => u16::from(p),
                    Value::U16(p) => p,
                    _ => unreachable!(),
                };

                if self.enable_io_operations {
                    match self.fetch_operand_value(&instruction, 1) {
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
            }

            iced_x86::Mnemonic::Outsb | iced_x86::Mnemonic::Outsw | iced_x86::Mnemonic::Outsd => {
                todo!()
            }

            iced_x86::Mnemonic::Pop => match self.operand_size(&instruction, 0) {
                1 => {
                    let val = Value::U8(self.stack_pop_u8());
                    self.store_in_operand(&instruction, 0, val);
                }
                2 => {
                    let val = Value::U16(self.stack_pop_u16());
                    self.store_in_operand(&instruction, 0, val);
                }
                4 => {
                    let val = Value::U32(self.stack_pop_u32());
                    self.store_in_operand(&instruction, 0, val);
                }
                _ => unreachable!(),
            },

            iced_x86::Mnemonic::Popa => match instruction.code() {
                iced_x86::Code::Popaw => {
                    let val = Value::U16(self.stack_pop_u16());
                    self.store_in_register(iced_x86::Register::DI, val);
                    let val = Value::U16(self.stack_pop_u16());
                    self.store_in_register(iced_x86::Register::SI, val);
                    let val = Value::U16(self.stack_pop_u16());
                    self.store_in_register(iced_x86::Register::BP, val);
                    let _ = self.stack_pop_u16();
                    let val = Value::U16(self.stack_pop_u16());
                    self.store_in_register(iced_x86::Register::BX, val);
                    let val = Value::U16(self.stack_pop_u16());
                    self.store_in_register(iced_x86::Register::DX, val);
                    let val = Value::U16(self.stack_pop_u16());
                    self.store_in_register(iced_x86::Register::CX, val);
                    let val = Value::U16(self.stack_pop_u16());
                    self.store_in_register(iced_x86::Register::AX, val);
                }
                iced_x86::Code::Popad => {
                    let val = Value::U32(self.stack_pop_u32());
                    self.store_in_register(iced_x86::Register::EDI, val);
                    let val = Value::U32(self.stack_pop_u32());
                    self.store_in_register(iced_x86::Register::ESI, val);
                    let val = Value::U32(self.stack_pop_u32());
                    self.store_in_register(iced_x86::Register::EBP, val);
                    let _ = self.stack_pop_u32();
                    let val = Value::U32(self.stack_pop_u32());
                    self.store_in_register(iced_x86::Register::EBX, val);
                    let val = Value::U32(self.stack_pop_u32());
                    self.store_in_register(iced_x86::Register::EDX, val);
                    let val = Value::U32(self.stack_pop_u32());
                    self.store_in_register(iced_x86::Register::ECX, val);
                    let val = Value::U32(self.stack_pop_u32());
                    self.store_in_register(iced_x86::Register::EAX, val);
                }
                _ => unreachable!(),
            },

            iced_x86::Mnemonic::Popf => match instruction.code() {
                iced_x86::Code::Popfw => {
                    let val = self.stack_pop_u16();
                    self.regs.flags &= 0b01000000000101010;
                    self.regs.flags |= val & 0b0111111111010101;
                }
                iced_x86::Code::Popfd => todo!(),
                _ => unreachable!(),
            },

            iced_x86::Mnemonic::Push => {
                let value = self.fetch_operand_value(&instruction, 0);
                self.stack_push_value(value);
            }

            iced_x86::Mnemonic::Pusha => match instruction.code() {
                iced_x86::Code::Pushaw => {
                    let sp = self.register(iced_x86::Register::SP);
                    self.stack_push_value(self.register(iced_x86::Register::AX));
                    self.stack_push_value(self.register(iced_x86::Register::CX));
                    self.stack_push_value(self.register(iced_x86::Register::DX));
                    self.stack_push_value(self.register(iced_x86::Register::BX));
                    self.stack_push_value(sp);
                    self.stack_push_value(self.register(iced_x86::Register::BP));
                    self.stack_push_value(self.register(iced_x86::Register::SI));
                    self.stack_push_value(self.register(iced_x86::Register::DI));
                }
                iced_x86::Code::Pushad => {
                    let esp = self.register(iced_x86::Register::ESP);
                    self.stack_push_value(self.register(iced_x86::Register::EAX));
                    self.stack_push_value(self.register(iced_x86::Register::ECX));
                    self.stack_push_value(self.register(iced_x86::Register::EDX));
                    self.stack_push_value(self.register(iced_x86::Register::EBX));
                    self.stack_push_value(esp);
                    self.stack_push_value(self.register(iced_x86::Register::EBP));
                    self.stack_push_value(self.register(iced_x86::Register::ESI));
                    self.stack_push_value(self.register(iced_x86::Register::EDI));
                }
                _ => unreachable!(),
            },

            iced_x86::Mnemonic::Pushf => match instruction.code() {
                iced_x86::Code::Pushfw => {
                    self.stack_push_value(Value::U16(self.regs.flags));
                }
                iced_x86::Code::Pushfd => todo!(),
                _ => unreachable!(),
            },

            iced_x86::Mnemonic::Rcl
            | iced_x86::Mnemonic::Rcr
            | iced_x86::Mnemonic::Rol
            | iced_x86::Mnemonic::Ror => {
                let mut value = self.fetch_operand_value(&instruction, 0);
                let count =
                    u8::try_from(self.fetch_operand_value(&instruction, 1)).unwrap() & 0b11111;
                let use_carry = matches!(
                    instruction.mnemonic(),
                    iced_x86::Mnemonic::Rcl | iced_x86::Mnemonic::Rcr
                );
                let dir_right = matches!(
                    instruction.mnemonic(),
                    iced_x86::Mnemonic::Rcr | iced_x86::Mnemonic::Ror
                );

                for _ in 0..count {
                    match (&mut value, dir_right) {
                        (Value::U8(v), false) => {
                            let bit = (*v >> 7) != 0;
                            *v = v.wrapping_mul(2);
                            let rotated = if use_carry {
                                self.flags_is_carry()
                            } else {
                                bit
                            };
                            *v |= if rotated { 1 } else { 0 };
                            self.flags_set_overflow(bit != (((*v >> 7) & 0x1) != 0));
                            self.flags_set_carry(bit);
                        }
                        (Value::U8(v), true) => {
                            let bit = (*v & 0x1) != 0;
                            *v = *v / 2;
                            let rotated = if use_carry {
                                self.flags_is_carry()
                            } else {
                                bit
                            };
                            *v |= if rotated { 1 } else { 0 } << 7;
                            self.flags_set_overflow(if use_carry {
                                self.flags_is_carry() != bit
                            } else {
                                bit != (((*v >> 6) & 0x1) != 0)
                            });
                            self.flags_set_carry(bit);
                        }
                        (Value::U16(v), false) => {
                            let bit = (*v >> 15) != 0;
                            *v = v.wrapping_mul(2);
                            let rotated = if use_carry {
                                self.flags_is_carry()
                            } else {
                                bit
                            };
                            *v |= if rotated { 1 } else { 0 };
                            self.flags_set_overflow(bit != (((*v >> 15) & 0x1) != 0));
                            self.flags_set_carry(bit);
                        }
                        (Value::U16(v), true) => {
                            let bit = (*v & 0x1) != 0;
                            *v = *v / 2;
                            let rotated = if use_carry {
                                self.flags_is_carry()
                            } else {
                                bit
                            };
                            *v |= if rotated { 1 } else { 0 } << 15;
                            self.flags_set_overflow(if use_carry {
                                self.flags_is_carry() != bit
                            } else {
                                bit != (((*v >> 14) & 0x1) != 0)
                            });
                            self.flags_set_carry(bit);
                        }
                        (Value::U32(v), false) => {
                            let bit = (*v >> 31) != 0;
                            *v = v.wrapping_mul(2);
                            let rotated = if use_carry {
                                self.flags_is_carry()
                            } else {
                                bit
                            };
                            *v |= if rotated { 1 } else { 0 };
                            self.flags_set_overflow(bit != (((*v >> 31) & 0x1) != 0));
                            self.flags_set_carry(bit);
                        }
                        (Value::U32(v), true) => {
                            let bit = (*v & 0x1) != 0;
                            *v = *v / 2;
                            let rotated = if use_carry {
                                self.flags_is_carry()
                            } else {
                                bit
                            };
                            *v |= if rotated { 1 } else { 0 } << 31;
                            self.flags_set_overflow(if use_carry {
                                self.flags_is_carry() != bit
                            } else {
                                bit != (((*v >> 30) & 0x1) != 0)
                            });
                            self.flags_set_carry(bit);
                        }
                    }
                }

                // OF is defined for some variants and undefined for others, so we don't
                // distinguish between these.
            }

            iced_x86::Mnemonic::Ret => {
                // The `ret` opcode can be followed by a number of bytes to pop from the stack
                // on top of `cs`/`ip`/`eip`.
                let num_to_pop = if instruction.op_count() == 1 {
                    self.fetch_operand_value(&instruction, 0)
                        .zero_extend_to_u32()
                } else {
                    0
                };

                match instruction.code() {
                    iced_x86::Code::Retnw_imm16 | iced_x86::Code::Retnw => {
                        let ip = self.stack_pop_u16();
                        self.regs.eip = u32::from(ip);
                    }
                    iced_x86::Code::Retfw_imm16 | iced_x86::Code::Retfw => {
                        let ip = self.stack_pop_u16();
                        self.regs.eip = u32::from(ip);
                        let cs = self.stack_pop_u16();
                        self.regs.cs = cs;
                    }
                    _ => unreachable!(),
                }

                for _ in 0..num_to_pop {
                    let _ = self.stack_pop_u8();
                }
            }

            iced_x86::Mnemonic::Sahf => {
                self.flags_set_sign(self.regs.eax & (1 << 7) != 0);
                self.flags_set_zero(self.regs.eax & (1 << 6) != 0);
                self.flags_set_adjust(self.regs.eax & (1 << 4) != 0);
                self.flags_set_parity(self.regs.eax & (1 << 2) != 0);
                self.flags_set_carry(self.regs.eax & (1 << 0) != 0);
            }

            iced_x86::Mnemonic::Sal
            | iced_x86::Mnemonic::Sar
            | iced_x86::Mnemonic::Shl
            | iced_x86::Mnemonic::Shr => {
                assert_eq!(instruction.op_count(), 2);
                let mut value0 = self.fetch_operand_value(&instruction, 0);
                let value1 = self.fetch_operand_value(&instruction, 1);

                for _ in 0..value1.zero_extend_to_u32() {
                    if matches!(
                        instruction.mnemonic(),
                        iced_x86::Mnemonic::Sal | iced_x86::Mnemonic::Shl
                    ) {
                        self.flags_set_carry(value0.most_significant_bit());
                    } else {
                        self.flags_set_carry(value0.least_significant_bit());
                    }

                    if matches!(
                        instruction.mnemonic(),
                        iced_x86::Mnemonic::Sal | iced_x86::Mnemonic::Shl
                    ) {
                        value0 = match value0 {
                            Value::U8(v) => Value::U8(v.wrapping_mul(2)),
                            Value::U16(v) => Value::U16(v.wrapping_mul(2)),
                            Value::U32(v) => Value::U32(v.wrapping_mul(2)),
                        };
                    } else if matches!(instruction.mnemonic(), iced_x86::Mnemonic::Sar) {
                        let sign_extension = value0.most_significant_bit();
                        value0 = match value0 {
                            Value::U8(v) => {
                                Value::U8((v / 2) | if sign_extension { 0x80 } else { 0 })
                            }
                            Value::U16(v) => {
                                Value::U16((v / 2) | if sign_extension { 0x8000 } else { 0 })
                            }
                            Value::U32(v) => {
                                Value::U32((v / 2) | if sign_extension { 0x80000000 } else { 0 })
                            }
                        };
                    } else {
                        value0 = match value0 {
                            Value::U8(v) => Value::U8(v / 2),
                            Value::U16(v) => Value::U16(v / 2),
                            Value::U32(v) => Value::U32(v / 2),
                        };
                    }
                }

                self.store_in_operand(&instruction, 0, value0);

                self.flags_set_sign_from_val(value0);
                self.flags_set_zero_from_val(value0);
                self.flags_set_parity_from_val(value0);
                // The adjust flag is undefined

                if matches!(
                    instruction.mnemonic(),
                    iced_x86::Mnemonic::Sal | iced_x86::Mnemonic::Shl
                ) {
                    self.flags_set_overflow(self.flags_is_carry() != value0.most_significant_bit());
                } else if matches!(instruction.mnemonic(), iced_x86::Mnemonic::Sar) {
                    self.flags_set_overflow(false);
                } else {
                    self.flags_set_overflow(value0.most_significant_bit());
                }
            }

            iced_x86::Mnemonic::Sbb => {
                let value0 = self.fetch_operand_value(&instruction, 0);
                let value1 = self.fetch_operand_value(&instruction, 1);

                let (temp, overflow) = match (value0, value1) {
                    (Value::U8(value0), Value::U8(value1)) => {
                        let carry_val = if self.flags_is_carry() { 1 } else { 0 };
                        let (v, o) = value0.overflowing_sub(value1.wrapping_add(carry_val));
                        (Value::U8(v), o)
                    }
                    (Value::U16(value0), Value::U16(value1)) => {
                        let carry_val = if self.flags_is_carry() { 1 } else { 0 };
                        let (v, o) = value0.overflowing_sub(value1.wrapping_add(carry_val));
                        (Value::U16(v), o)
                    }
                    (Value::U32(value0), Value::U32(value1)) => {
                        let carry_val = if self.flags_is_carry() { 1 } else { 0 };
                        let (v, o) = value0.overflowing_sub(value1.wrapping_add(carry_val));
                        (Value::U32(v), o)
                    }
                    _ => unreachable!(),
                };

                self.store_in_operand(&instruction, 0, temp);

                self.flags_set_sign_from_val(temp);
                self.flags_set_zero_from_val(temp);
                self.flags_set_parity_from_val(temp);
                self.flags_set_carry(overflow);
                self.flags_set_overflow(overflow != temp.most_significant_bit());
                // TODO: the adjust flag
            }

            iced_x86::Mnemonic::Scasb | iced_x86::Mnemonic::Scasw | iced_x86::Mnemonic::Scasd => {
                assert_eq!(instruction.op_count(), 2);
                let value0 = self.fetch_operand_value(&instruction, 0);
                let value1 = self.fetch_operand_value(&instruction, 1);

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

                self.flags_set_sign_from_val(temp);
                self.flags_set_zero_from_val(temp);
                self.flags_set_parity_from_val(temp);
                self.flags_set_carry(overflow);
                self.flags_set_overflow(overflow != temp.most_significant_bit());
                // TODO: the adjust flag

                let use_edi = match instruction.op_kind(1) {
                    iced_x86::OpKind::MemorySegDI => false,
                    iced_x86::OpKind::MemorySegEDI => true,
                    iced_x86::OpKind::MemoryESDI => false,
                    iced_x86::OpKind::MemoryESEDI => true,
                    _ => unreachable!(),
                };

                if use_edi {
                    if self.flags_is_direction() {
                        self.regs.edi = self.regs.edi.wrapping_sub(u32::from(temp.size()));
                    } else {
                        self.regs.edi = self.regs.edi.wrapping_add(u32::from(temp.size()));
                    }
                } else {
                    if self.flags_is_direction() {
                        self.sub_di(u16::from(temp.size()));
                    } else {
                        self.add_di(u16::from(temp.size()));
                    }
                }
            }

            iced_x86::Mnemonic::Seta
            | iced_x86::Mnemonic::Setae
            | iced_x86::Mnemonic::Setb
            | iced_x86::Mnemonic::Setbe
            | iced_x86::Mnemonic::Sete
            | iced_x86::Mnemonic::Setg
            | iced_x86::Mnemonic::Setge
            | iced_x86::Mnemonic::Setl
            | iced_x86::Mnemonic::Setle
            | iced_x86::Mnemonic::Setne
            | iced_x86::Mnemonic::Setno
            | iced_x86::Mnemonic::Setnp
            | iced_x86::Mnemonic::Setns
            | iced_x86::Mnemonic::Seto
            | iced_x86::Mnemonic::Setp
            | iced_x86::Mnemonic::Sets => {
                debug_assert!(!matches!(
                    instruction.condition_code(),
                    iced_x86::ConditionCode::None
                ));
                let cond = self.flags_check_condition(instruction.condition_code());
                let value = Value::U8(if cond { 1 } else { 0 });
                self.store_in_operand(&instruction, 0, value);
            }

            iced_x86::Mnemonic::Shld => todo!(),
            iced_x86::Mnemonic::Shrd => todo!(),

            iced_x86::Mnemonic::Smsw => todo!(),

            iced_x86::Mnemonic::Stc => self.flags_set_carry(true),
            iced_x86::Mnemonic::Std => self.flags_set_direction(true),
            iced_x86::Mnemonic::Sti => self.flags_set_interrupt(true),

            iced_x86::Mnemonic::Stosb | iced_x86::Mnemonic::Stosw | iced_x86::Mnemonic::Stosd => {
                // TODO: review this
                let val = self.fetch_operand_value(&instruction, 1);

                let use_edi = match instruction.op_kind(0) {
                    iced_x86::OpKind::MemorySegDI => false,
                    iced_x86::OpKind::MemorySegEDI => true,
                    iced_x86::OpKind::MemoryESDI => false,
                    iced_x86::OpKind::MemoryESEDI => true,
                    _ => unreachable!(),
                };

                self.store_in_operand(&instruction, 0, val);

                if use_edi {
                    if self.flags_is_direction() {
                        self.regs.edi = self.regs.edi.wrapping_sub(u32::from(val.size()));
                    } else {
                        self.regs.edi = self.regs.edi.wrapping_add(u32::from(val.size()));
                    }
                } else {
                    if self.flags_is_direction() {
                        self.sub_di(u16::from(val.size()));
                    } else {
                        self.add_di(u16::from(val.size()));
                    }
                }
            }

            iced_x86::Mnemonic::Sub => {
                let value0 = self.fetch_operand_value(&instruction, 0);
                let value1 = self.fetch_operand_value(&instruction, 1);

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

                self.store_in_operand(&instruction, 0, temp);

                self.flags_set_sign_from_val(temp);
                self.flags_set_zero_from_val(temp);
                self.flags_set_parity_from_val(temp);
                self.flags_set_carry(overflow);
                self.flags_set_overflow(overflow != temp.most_significant_bit());
                // TODO: the adjust flag
            }

            iced_x86::Mnemonic::Test => {
                let value0 = self.fetch_operand_value(&instruction, 0);
                let value1 = self.fetch_operand_value(&instruction, 1);

                let temp = match (value0, value1) {
                    (Value::U8(value0), Value::U8(value1)) => Value::U8(value0 & value1),
                    (Value::U16(value0), Value::U16(value1)) => Value::U16(value0 & value1),
                    (Value::U32(value0), Value::U32(value1)) => Value::U32(value0 & value1),
                    _ => unreachable!(),
                };

                self.flags_set_sign_from_val(temp);
                self.flags_set_zero_from_val(temp);
                self.flags_set_parity_from_val(temp);
                self.flags_set_carry(false);
                self.flags_set_overflow(false);
                // adjust flag is undefined
            }

            iced_x86::Mnemonic::Wait => {}

            iced_x86::Mnemonic::Xchg => {
                let value0 = self.fetch_operand_value(&instruction, 0);
                let value1 = self.fetch_operand_value(&instruction, 1);
                self.store_in_operand(&instruction, 0, value1);
                self.store_in_operand(&instruction, 1, value0);
            }

            iced_x86::Mnemonic::Xlatb => todo!(),

            iced_x86::Mnemonic::INVALID => return Err(Error::InvalidInstruction),
            opcode => {
                log::error!("Unsupported instruction: {:?}", opcode);
                return Err(Error::InvalidInstruction);
            }
        }

        Ok(())
    }

    /// Executes the `int` instruction on the current state of the machine.
    fn run_int_opcode(&mut self, vector: u8) {
        self.stack_push_value(Value::U16(self.regs.flags));
        self.stack_push_value(Value::U16(self.regs.cs));
        self.stack_push_value(Value::U16(self.ip()));

        self.flags_set_interrupt(false);
        self.flags_set_trap(false);

        let vector = u32::from(vector);

        self.regs.cs = self.read_memory_u16((vector * 4) + 2);
        self.regs.eip = u32::from(self.read_memory_u16(vector * 4));
    }

    /// Unconditionally applies the jump instruction passed as parameter.
    ///
    /// > **Note**: Does not work with `int` instruction.
    ///
    /// # Panic
    ///
    /// Panics if the passed instruction is not a jump or a call.
    ///
    fn apply_jump(&mut self, instruction: &iced_x86::Instruction) {
        assert_eq!(instruction.op_count(), 1);

        match instruction.op_kind(0) {
            iced_x86::OpKind::NearBranch16 => {
                self.regs.eip = u32::from(instruction.near_branch16());
            }
            iced_x86::OpKind::NearBranch32 => {
                self.regs.eip = instruction.near_branch32();
            }
            iced_x86::OpKind::FarBranch16 => {
                self.regs.cs = instruction.far_branch_selector();
                self.regs.eip = u32::from(instruction.far_branch16());
            }
            iced_x86::OpKind::FarBranch32 => {
                self.regs.cs = instruction.far_branch_selector();
                self.regs.eip = instruction.far_branch32();
            }
            _ => {
                self.regs.eip = match self.fetch_operand_value(instruction, 0) {
                    Value::U16(v) => u32::from(v),
                    Value::U32(v) => v,
                    _ => unreachable!(),
                };
            }
        }
    }

    /// Pushes data on the stack.
    fn stack_push(&mut self, data: &[u8]) {
        // TODO: don't panic; handle overflows by generating a SS exception
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
        self.flags_set_sign(val.most_significant_bit())
    }

    fn flags_set_sign(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 7;
        } else {
            self.regs.flags &= !(1 << 7);
        }
    }

    fn flags_set_trap(&mut self, val: bool) {
        if val {
            self.regs.flags |= 1 << 8;
        } else {
            self.regs.flags &= !(1 << 8);
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

    /// Checks whether the state of the flags matches the given condition. Returns `true` if `None`
    /// is passed.
    fn flags_check_condition(&self, condition: iced_x86::ConditionCode) -> bool {
        match condition {
            iced_x86::ConditionCode::None => true,
            iced_x86::ConditionCode::o => self.flags_is_overflow(),
            iced_x86::ConditionCode::no => !self.flags_is_overflow(),
            iced_x86::ConditionCode::b => self.flags_is_carry(),
            iced_x86::ConditionCode::ae => !self.flags_is_carry(),
            iced_x86::ConditionCode::e => self.flags_is_zero(),
            iced_x86::ConditionCode::ne => !self.flags_is_zero(),
            iced_x86::ConditionCode::be => self.flags_is_carry() || self.flags_is_zero(),
            iced_x86::ConditionCode::a => !self.flags_is_carry() && !self.flags_is_zero(),
            iced_x86::ConditionCode::s => self.flags_is_sign(),
            iced_x86::ConditionCode::ns => self.flags_is_sign(),
            iced_x86::ConditionCode::p => self.flags_is_parity(),
            iced_x86::ConditionCode::np => self.flags_is_parity(),
            iced_x86::ConditionCode::l => self.flags_is_sign() != self.flags_is_zero(),
            iced_x86::ConditionCode::ge => self.flags_is_sign() == self.flags_is_zero(),
            iced_x86::ConditionCode::le => {
                self.flags_is_zero() || self.flags_is_sign() != self.flags_is_overflow()
            }
            iced_x86::ConditionCode::g => {
                !self.flags_is_zero() && self.flags_is_sign() == self.flags_is_overflow()
            }
        }
    }

    /// Returns the value of the `IP` register.
    fn ip(&self) -> u16 {
        u16::try_from(self.regs.eip & 0xffff).unwrap()
    }

    fn dec_cx(&mut self) {
        let cx = u16::try_from(self.regs.ecx & 0xffff).unwrap();
        let new_cx = cx.wrapping_sub(1);
        self.regs.ecx &= 0xffff0000;
        self.regs.ecx |= u32::from(new_cx);
    }

    fn sub_si(&mut self, n: u16) {
        let si = u16::try_from(self.regs.esi & 0xffff).unwrap();
        let new_si = si.wrapping_sub(n);
        self.regs.esi &= 0xffff0000;
        self.regs.esi |= u32::from(new_si);
    }

    fn add_si(&mut self, n: u16) {
        let si = u16::try_from(self.regs.esi & 0xffff).unwrap();
        let new_si = si.wrapping_add(n);
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
    ///
    /// # Panic
    ///
    /// Panics if the operand is not of type `Memory`.
    ///
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

    /// Returns the size in bytes of the value designated by the given operand of the given
    /// instruction. This is equal to what [`Interpreter::fetch_operand_value`] would return
    /// for that operand.
    ///
    /// For example if the operand is the register `AX`, returns 2.
    ///
    /// # Panic
    ///
    /// Panics if the operand index is out of range of the instruction.
    ///
    fn operand_size(&mut self, instruction: &iced_x86::Instruction, op_n: u32) -> u8 {
        match instruction.op_kind(op_n) {
            // TODO: lazy way to implement this
            iced_x86::OpKind::Register => {
                debug_assert!(!matches!(
                    instruction.op_register(op_n),
                    iced_x86::Register::None
                ));
                self.register(instruction.op_register(op_n)).size()
            }
            iced_x86::OpKind::Immediate8 => 1,
            iced_x86::OpKind::Immediate16 => 2,
            iced_x86::OpKind::Immediate32 => 2,
            iced_x86::OpKind::Immediate8to16 => 2,
            iced_x86::OpKind::Immediate8to32 => 4,
            iced_x86::OpKind::MemorySegSI
            | iced_x86::OpKind::MemoryESDI
            | iced_x86::OpKind::Memory => u8::try_from(instruction.memory_size().size()).unwrap(),
            ty => unimplemented!("{:?}", ty),
        }
    }

    /// Returns the value of the given operand of the given instruction.
    ///
    /// For example if the operand is the register `AX`, returns the current value of `AX`.
    ///
    /// # Panic
    ///
    /// Panics if the operand index is out of range of the instruction.
    ///
    fn fetch_operand_value(&mut self, instruction: &iced_x86::Instruction, op_n: u32) -> Value {
        let (segment, pointer) = match instruction.op_kind(op_n) {
            iced_x86::OpKind::Register => {
                debug_assert!(
                    !matches!(instruction.op_register(op_n), iced_x86::Register::None),
                    "{:?} with {:?}",
                    instruction.code(),
                    op_n
                );
                return self.register(instruction.op_register(op_n));
            }
            iced_x86::OpKind::Immediate8 => {
                if (0..op_n).any(|n| instruction.op_kind(n) == iced_x86::OpKind::Immediate8) {
                    return Value::U8(instruction.immediate8_2nd());
                } else {
                    return Value::U8(instruction.immediate8());
                }
            }
            iced_x86::OpKind::Immediate16 => return Value::U16(instruction.immediate16()),
            iced_x86::OpKind::Immediate32 => return Value::U32(instruction.immediate32()),
            iced_x86::OpKind::Immediate8to16 => {
                return Value::U16(u16::from_ne_bytes(
                    instruction.immediate8to16().to_ne_bytes(),
                ))
            }
            iced_x86::OpKind::Immediate8to32 => {
                return Value::U32(u32::from_ne_bytes(
                    instruction.immediate8to32().to_ne_bytes(),
                ))
            }
            iced_x86::OpKind::MemorySegSI => {
                let segment = {
                    debug_assert!(!matches!(
                        instruction.memory_segment(),
                        iced_x86::Register::None
                    ));
                    u16::try_from(self.register(instruction.memory_segment())).unwrap()
                };
                let pointer = u16::try_from(self.regs.esi & 0xffff).unwrap();
                (segment, pointer)
            }
            iced_x86::OpKind::MemoryESDI => {
                let segment = u16::try_from(self.regs.es).unwrap();
                let pointer = u16::try_from(self.regs.edi & 0xffff).unwrap();
                (segment, pointer)
            }
            iced_x86::OpKind::Memory => {
                let segment = {
                    debug_assert!(!matches!(
                        instruction.memory_segment(),
                        iced_x86::Register::None
                    ));
                    u16::try_from(self.register(instruction.memory_segment())).unwrap()
                };
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

    /// Returns the value of the given register.
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

    /// Assumes that the given operand of the given instruction is either a register or a memory
    /// address, and stores the given value at this location.
    ///
    /// # Panic
    ///
    /// Panics if the operand index is invalid for this instruction.
    /// Panics if the operand designates a register and this register is of the wrong size.
    ///
    fn store_in_operand(&mut self, instruction: &iced_x86::Instruction, op_n: u32, val: Value) {
        let (segment, pointer) = match instruction.op_kind(op_n) {
            iced_x86::OpKind::Register => {
                return self.store_in_register(instruction.op_register(op_n), val);
            }
            iced_x86::OpKind::MemoryESDI => {
                (self.regs.es, u16::try_from(self.regs.edi & 0xffff).unwrap())
            }
            iced_x86::OpKind::Memory => {
                debug_assert!(!matches!(
                    instruction.memory_segment(),
                    iced_x86::Register::None
                ));
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

    /// Stores the given value in the given register.
    ///
    /// # Panic
    ///
    /// Panics if the register and valure are not the same size.
    /// Panics if `register` is `CS`. Writing in `CS` is explicitly forbidden.
    ///
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

    fn most_significant_bit(&self) -> bool {
        match *self {
            Value::U8(val) => (val & 0x80) != 0,
            Value::U16(val) => (val & 0x8000) != 0,
            Value::U32(val) => (val & 0x80000000) != 0,
        }
    }

    fn least_significant_bit(&self) -> bool {
        match *self {
            Value::U8(val) => (val & 0x1) != 0,
            Value::U16(val) => (val & 0x1) != 0,
            Value::U32(val) => (val & 0x1) != 0,
        }
    }

    fn zero_extend_to_u32(&self) -> u32 {
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

    fn is_max_value(&self) -> bool {
        match *self {
            Value::U8(val) => val == 0xff,
            Value::U16(val) => val == 0xffff,
            Value::U32(val) => val == 0xffffffff,
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
