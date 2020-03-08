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

//! Programmable Interrupt Timer (PIT)
//!
//! The PIT is a chip that allows raising an Interrupt ReQuest (IRQ) after a certain time has
//! elapsed. This IRQ is propagated to [the I/O APIC], which then delivers an interrupt to one
//! or more processors.
//!
//! In order to determine which IRQ is raised, one need to look at the interrupt source overrides
//! of the ACPI tables for an entry corresponding to ISA IRQ 0.
//!
//! # About performances
//!
//! The implementation below is very inefficient and very restrictive. The PIT in general
//! should only be used in limited circumstances and not under regular load.
//!

use crate::arch::x86_64::{
    apic::{ioapics, local},
    interrupts,
};

use alloc::sync::Arc;
use core::{
    convert::TryFrom as _,
    future::Future,
    mem,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
    time::Duration,
};
use futures::task::ArcWake;
use x86_64::structures::port::PortWrite as _;

pub struct PitControl {
    /// Reservation for an interrupt vector in the table.
    interrupt_vector: interrupts::ReservedInterruptVector,
}

/// Future created with [`PitControl::timer`].
pub struct PitFuture<'a> {
    pit: &'a mut PitControl,

    /// If `true`, then the PIT interrupt has fired and we must decrease the number of
    /// ticks the next time we're polled.
    raised: Arc<AtomicBool>,

    /// We must first wait this value times `u16::max_value()` ticks.
    num_major_ticks: u128,

    /// After [`PitFuture::num_major_ticks`] is zero, we must wait an additional
    /// number of ticks indicated below.
    minor_modulus: u16,
}

/// Initializes the PIT.
///
/// There should only ever be one [`PitControl`] alive at any given point in time. Creating
/// multiple [`PitControl`] is safe, but will lead to logic errors.
pub fn init_pit(
    local_apics: &local::LocalApicsControl,
    io_apics: &mut ioapics::IoApicsControl,
) -> PitControl {
    let interrupt_vector = interrupts::reserve_any_vector().unwrap();
    io_apics.isa_irq(0).unwrap().set_destination(
        local_apics.current_apic_id(),
        interrupt_vector.interrupt_num(),
    );

    PitControl { interrupt_vector }
}

impl PitControl {
    /// Creates a new future that will trigger when the given amount of time has elapsed.
    ///
    /// The implementation only allows one timer at a time, thus the future mutably borrows
    /// the PIT.
    ///
    /// You must call [`interrupts::load_idt`] on at least one CPU for the timer to work.
    pub fn timer(&mut self, after: Duration) -> PitFuture {
        // Calculate the number of ticks that we will for the PIT to produce before the duration
        // has elapsed.
        // TODO: probably rounding errors below
        const TICKS_PER_SEC: u64 = 1_193_182;
        const NANOS_PER_TICK: u128 = 1 + 1_000_000_000 / TICKS_PER_SEC as u128;
        let num_ticks = 1 + after.as_nanos() / NANOS_PER_TICK;

        // The PIT only accepts a 16bits number of ticks, and `num_ticks` probably won't fit
        // in 16bits. We therefore do some euclidian division and remainder.
        // The `Future` will need to be awakened `num_major_ticks` times.
        let num_major_ticks = num_ticks / u128::from(u16::max_value());
        let minor_modulus = u16::try_from(num_ticks % u128::from(u16::max_value())).unwrap();

        PitFuture {
            pit: self,
            raised: Arc::new(AtomicBool::new(true)),
            num_major_ticks,
            minor_modulus,
        }
    }
}

impl<'a> Future for PitFuture<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        if !self.raised.swap(false, Ordering::Relaxed) {
            return Poll::Pending;
        }

        let num_ticks = if self.num_major_ticks == 0 {
            mem::replace(&mut self.minor_modulus, 0)
        } else {
            self.num_major_ticks -= 1;
            u16::max_value()
        };

        if num_ticks == 0 {
            return Poll::Ready(());
        }

        // TODO: rather than register a Waker, put an implementation of Future that is raised
        // only when an interrupt has actually triggered
        self.pit
            .interrupt_vector
            .register_waker(&futures::task::waker(Arc::new(RaisingArc {
                inner: cx.waker().clone(),
                raised: self.raised.clone(),
            })));

        channel0_one_shot(num_ticks);
        Poll::Pending
    }
}

/// Instructs the PIT to trigger an IRQ0 after the specified number of ticks have elapsed.
/// The tick frequency is approximately equal to 1.193182 MHz.
fn channel0_one_shot(ticks: u16) {
    unsafe {
        // Set channel 0 to "interrupt on terminal count" mode and prepare for writing the value.
        u8::write_to_port(0x43, 0b00110000);

        let bytes = ticks.to_le_bytes();
        u8::write_to_port(0x40, bytes[0]);
        u8::write_to_port(0x40, bytes[1]);
    }
}

/// Implementation of `ArcWake` that additionally sets `raised` to true when it is awakened.
struct RaisingArc {
    inner: Waker,
    raised: Arc<AtomicBool>,
}

impl ArcWake for RaisingArc {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let _was_raised = arc_self.raised.swap(true, Ordering::Relaxed);
        // If `_was_raised` is true here, then we will miss a interrupt. We prefer to panic
        // rather than miss an interrupt.
        assert!(!_was_raised);
        arc_self.inner.wake_by_ref();
    }
}
