// Copyright (C) 2019  Pierre Krieger
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

//! x86_64 interrupts handling.
//!
//! This module provides handling.of interrupts on x86_64. It sets up the (IDT) interrupts table
//! and allows registers a [`Waker`](core::task::Waker) that is waken up when an interrupt happens.
//! This is done by calling [`set_interrupt_waker`].
//!
//! Note that this API is racy. Once a `Waker` has been woken up, it gets discarded and needs to
//! be registered again. It is possible that an interrupt gets triggered between the discard and
//! the re-registration.
//!
//! This is not considered to be a problem, as hardware normally lets you know why an interrupt
//! has happened. By re-registering a `Waker` before looking for the interrupt reason, there is no
//! risk of losing information.
//!

// TODO: init() has to be called; this isn't great

use core::task::Waker;
use futures::task::AtomicWaker;
use x86_64::structures::idt;

/// Registers a `Waker` to wake up when an interrupt happens.
///
/// For each value of `interrupt`, only the latest registered `Waker` will be waken up.
///
/// > **Note**: Interrupts 8 and 18 are considered unrecoverable, and it therefore doesn't make
/// >           sense to call this method with `interrupt` equal to 8 or 18.
///
pub fn set_interrupt_waker(interrupt: u8, waker: &Waker) {
    debug_assert_ne!(interrupt, 8);
    debug_assert_ne!(interrupt, 18);
    WAKERS[usize::from(interrupt)].register(waker);
}

/// Initializes the interrupts system.
///
/// Before this is called, the waker passed to [`set_interrupt_waker`] will never work.
///
/// # Safety
///
/// Not thread safe. Only call once at a time.
///
pub unsafe fn init() {
    macro_rules! set_entry {
        ($idt:ident[$n:expr]) => {{
            set_entry!($idt[$n], $n);
        }};
        ($entry:expr, $n:expr) => {{
            extern "x86-interrupt" fn handler(_: &mut idt::InterruptStackFrame) {
                WAKERS[$n].wake();
            }
            $entry.set_handler_fn(handler)
                .disable_interrupts(false);
        }};
        ($entry:expr, $n:expr, with-err) => {{
            extern "x86-interrupt" fn handler(_: &mut idt::InterruptStackFrame, _: u64) {
                WAKERS[$n].wake();
            }
            $entry.set_handler_fn(handler)
                .disable_interrupts(false);
        }};
        ($entry:expr, $n:expr, halt-with-err) => {{
            extern "x86-interrupt" fn handler(_: &mut idt::InterruptStackFrame, _: u64) {
                x86_64::instructions::interrupts::disable();
                x86_64::instructions::bochs_breakpoint();
                x86_64::instructions::hlt();
            }
            $entry.set_handler_fn(handler);
        }};
        ($entry:expr, $n:expr, with-pf-err) => {{
            extern "x86-interrupt" fn handler(_: &mut idt::InterruptStackFrame, _: idt::PageFaultErrorCode) {
                WAKERS[$n].wake();
            }
            $entry.set_handler_fn(handler)
                .disable_interrupts(false);
        }};
        ($entry:expr, $n:expr, diverging) => {{
            extern "x86-interrupt" fn handler(_: &mut idt::InterruptStackFrame) -> ! {
                panic!()
            }
            $entry.set_handler_fn(handler);
        }};
        ($entry:expr, $n:expr, diverging-with-err) => {{
            extern "x86-interrupt" fn handler(_: &mut idt::InterruptStackFrame, _: u64) -> ! {
                panic!("double fault!") // TODO: well, this is supposedly a generic diverging interrupt handler
            }
            $entry.set_handler_fn(handler);
        }};
    }

    set_entry!(IDT[0]);
    set_entry!(IDT[1]);
    set_entry!(IDT[2]);
    set_entry!(IDT[3]);
    set_entry!(IDT[4]);
    set_entry!(IDT[5]);
    set_entry!(IDT[6]);
    set_entry!(IDT[7]);
    set_entry!(IDT.double_fault, 8, diverging - with - err);
    set_entry!(IDT[9]);
    set_entry!(IDT.invalid_tss, 10, with - err);
    set_entry!(IDT.segment_not_present, 11, with - err);
    set_entry!(IDT.stack_segment_fault, 12, with - err);
    set_entry!(IDT.general_protection_fault, 13, halt - with - err);
    set_entry!(IDT.page_fault, 14, with - pf - err);
    // 15 is reserved
    set_entry!(IDT[16]);
    set_entry!(IDT.alignment_check, 17, with - err);
    set_entry!(IDT.machine_check, 18, diverging);
    set_entry!(IDT[19]);
    set_entry!(IDT[20]);
    // 21 is reserved
    // 22 is reserved
    // 23 is reserved
    // 24 is reserved
    // 25 is reserved
    // 26 is reserved
    // 27 is reserved
    // 28 is reserved
    // 29 is reserved
    set_entry!(IDT.security_exception, 30, with - err);
    // 31 is reserved
    set_entry!(IDT[32]);
    set_entry!(IDT[33]);
    set_entry!(IDT[34]);
    set_entry!(IDT[35]);
    set_entry!(IDT[36]);
    set_entry!(IDT[37]);
    set_entry!(IDT[38]);
    set_entry!(IDT[39]);
    set_entry!(IDT[40]);
    set_entry!(IDT[41]);
    set_entry!(IDT[42]);
    set_entry!(IDT[43]);
    set_entry!(IDT[44]);
    set_entry!(IDT[45]);
    set_entry!(IDT[46]);
    set_entry!(IDT[47]);
    set_entry!(IDT[48]);
    set_entry!(IDT[49]);
    set_entry!(IDT[50]);
    set_entry!(IDT[51]);
    set_entry!(IDT[52]);
    set_entry!(IDT[53]);
    set_entry!(IDT[54]);
    set_entry!(IDT[55]);
    set_entry!(IDT[56]);
    set_entry!(IDT[57]);
    set_entry!(IDT[58]);
    set_entry!(IDT[59]);
    set_entry!(IDT[60]);
    set_entry!(IDT[61]);
    set_entry!(IDT[62]);
    set_entry!(IDT[63]);
    set_entry!(IDT[64]);
    set_entry!(IDT[65]);
    set_entry!(IDT[66]);
    set_entry!(IDT[67]);
    set_entry!(IDT[68]);
    set_entry!(IDT[69]);
    set_entry!(IDT[70]);
    set_entry!(IDT[71]);
    set_entry!(IDT[72]);
    set_entry!(IDT[73]);
    set_entry!(IDT[74]);
    set_entry!(IDT[75]);
    set_entry!(IDT[76]);
    set_entry!(IDT[77]);
    set_entry!(IDT[78]);
    set_entry!(IDT[79]);
    set_entry!(IDT[80]);
    set_entry!(IDT[81]);
    set_entry!(IDT[82]);
    set_entry!(IDT[83]);
    set_entry!(IDT[84]);
    set_entry!(IDT[85]);
    set_entry!(IDT[86]);
    set_entry!(IDT[87]);
    set_entry!(IDT[88]);
    set_entry!(IDT[89]);
    set_entry!(IDT[90]);
    set_entry!(IDT[91]);
    set_entry!(IDT[92]);
    set_entry!(IDT[93]);
    set_entry!(IDT[94]);
    set_entry!(IDT[95]);
    set_entry!(IDT[96]);
    set_entry!(IDT[97]);
    set_entry!(IDT[98]);
    set_entry!(IDT[99]);
    set_entry!(IDT[100]);
    set_entry!(IDT[101]);
    set_entry!(IDT[102]);
    set_entry!(IDT[103]);
    set_entry!(IDT[104]);
    set_entry!(IDT[105]);
    set_entry!(IDT[106]);
    set_entry!(IDT[107]);
    set_entry!(IDT[108]);
    set_entry!(IDT[109]);
    set_entry!(IDT[110]);
    set_entry!(IDT[111]);
    set_entry!(IDT[112]);
    set_entry!(IDT[113]);
    set_entry!(IDT[114]);
    set_entry!(IDT[115]);
    set_entry!(IDT[116]);
    set_entry!(IDT[117]);
    set_entry!(IDT[118]);
    set_entry!(IDT[119]);
    set_entry!(IDT[120]);
    set_entry!(IDT[121]);
    set_entry!(IDT[122]);
    set_entry!(IDT[123]);
    set_entry!(IDT[124]);
    set_entry!(IDT[125]);
    set_entry!(IDT[126]);
    set_entry!(IDT[127]);
    set_entry!(IDT[128]);
    set_entry!(IDT[129]);
    set_entry!(IDT[130]);
    set_entry!(IDT[131]);
    set_entry!(IDT[132]);
    set_entry!(IDT[133]);
    set_entry!(IDT[134]);
    set_entry!(IDT[135]);
    set_entry!(IDT[136]);
    set_entry!(IDT[137]);
    set_entry!(IDT[138]);
    set_entry!(IDT[139]);
    set_entry!(IDT[140]);
    set_entry!(IDT[141]);
    set_entry!(IDT[142]);
    set_entry!(IDT[143]);
    set_entry!(IDT[144]);
    set_entry!(IDT[145]);
    set_entry!(IDT[146]);
    set_entry!(IDT[147]);
    set_entry!(IDT[148]);
    set_entry!(IDT[149]);
    set_entry!(IDT[150]);
    set_entry!(IDT[151]);
    set_entry!(IDT[152]);
    set_entry!(IDT[153]);
    set_entry!(IDT[154]);
    set_entry!(IDT[155]);
    set_entry!(IDT[156]);
    set_entry!(IDT[157]);
    set_entry!(IDT[158]);
    set_entry!(IDT[159]);
    set_entry!(IDT[160]);
    set_entry!(IDT[161]);
    set_entry!(IDT[162]);
    set_entry!(IDT[163]);
    set_entry!(IDT[164]);
    set_entry!(IDT[165]);
    set_entry!(IDT[166]);
    set_entry!(IDT[167]);
    set_entry!(IDT[168]);
    set_entry!(IDT[169]);
    set_entry!(IDT[170]);
    set_entry!(IDT[171]);
    set_entry!(IDT[172]);
    set_entry!(IDT[173]);
    set_entry!(IDT[174]);
    set_entry!(IDT[175]);
    set_entry!(IDT[176]);
    set_entry!(IDT[177]);
    set_entry!(IDT[178]);
    set_entry!(IDT[179]);
    set_entry!(IDT[180]);
    set_entry!(IDT[181]);
    set_entry!(IDT[182]);
    set_entry!(IDT[183]);
    set_entry!(IDT[184]);
    set_entry!(IDT[185]);
    set_entry!(IDT[186]);
    set_entry!(IDT[187]);
    set_entry!(IDT[188]);
    set_entry!(IDT[189]);
    set_entry!(IDT[190]);
    set_entry!(IDT[191]);
    set_entry!(IDT[192]);
    set_entry!(IDT[193]);
    set_entry!(IDT[194]);
    set_entry!(IDT[195]);
    set_entry!(IDT[196]);
    set_entry!(IDT[197]);
    set_entry!(IDT[198]);
    set_entry!(IDT[199]);
    set_entry!(IDT[200]);
    set_entry!(IDT[201]);
    set_entry!(IDT[202]);
    set_entry!(IDT[203]);
    set_entry!(IDT[204]);
    set_entry!(IDT[205]);
    set_entry!(IDT[206]);
    set_entry!(IDT[207]);
    set_entry!(IDT[208]);
    set_entry!(IDT[209]);
    set_entry!(IDT[210]);
    set_entry!(IDT[211]);
    set_entry!(IDT[212]);
    set_entry!(IDT[213]);
    set_entry!(IDT[214]);
    set_entry!(IDT[215]);
    set_entry!(IDT[216]);
    set_entry!(IDT[217]);
    set_entry!(IDT[218]);
    set_entry!(IDT[219]);
    set_entry!(IDT[220]);
    set_entry!(IDT[221]);
    set_entry!(IDT[222]);
    set_entry!(IDT[223]);
    set_entry!(IDT[224]);
    set_entry!(IDT[225]);
    set_entry!(IDT[226]);
    set_entry!(IDT[227]);
    set_entry!(IDT[228]);
    set_entry!(IDT[229]);
    set_entry!(IDT[230]);
    set_entry!(IDT[231]);
    set_entry!(IDT[232]);
    set_entry!(IDT[233]);
    set_entry!(IDT[234]);
    set_entry!(IDT[235]);
    set_entry!(IDT[236]);
    set_entry!(IDT[237]);
    set_entry!(IDT[238]);
    set_entry!(IDT[239]);
    set_entry!(IDT[240]);
    set_entry!(IDT[241]);
    set_entry!(IDT[242]);
    set_entry!(IDT[243]);
    set_entry!(IDT[244]);
    set_entry!(IDT[245]);
    set_entry!(IDT[246]);
    set_entry!(IDT[247]);
    set_entry!(IDT[248]);
    set_entry!(IDT[249]);
    set_entry!(IDT[250]);
    set_entry!(IDT[251]);
    set_entry!(IDT[252]);
    set_entry!(IDT[253]);
    set_entry!(IDT[254]);
    set_entry!(IDT[255]);

    IDT.load();
    x86_64::instructions::interrupts::enable();
}

/// Table read by the hardware in order to determine what to do when an interrupt happens.
static mut IDT: idt::InterruptDescriptorTable = idt::InterruptDescriptorTable::new();

/// For each interrupt vector, a [`Waker`](core::task::Waker) that must be waken up when that
/// interrupt happens.
static WAKERS: [AtomicWaker; 256] = [
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
    AtomicWaker::new(),
];
