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

//! Futures executor that works on bare metal.

use crate::arch::x86_64::{
    apic::{local::LocalApicsControl, ApicId},
    interrupts,
};

use alloc::sync::Arc;
use core::future::Future;
use core::sync::atomic;
use core::task::{Context, Poll};
use futures::task::{waker, ArcWake};

// TODO: we use SeqCst everywhere, but we can probably use a better ordering

/// Contains all the necessary information to executor futures.
pub struct Executor {
    /// Reserved interrupt vector. Used to wake up other processors.
    interrupt_vector: interrupts::ReservedInterruptVector,

    /// The local APIC, to send IPIs.
    apic: &'static LocalApicsControl,
}

impl Executor {
    /// Initializes a new [`Executor`].
    pub fn new(local_apic: &'static LocalApicsControl) -> Self {
        Executor {
            interrupt_vector: interrupts::reserve_any_vector(true).unwrap(),
            apic: local_apic,
        }
    }

    /// Waits for the `Future` to resolve to a value.
    ///
    /// This function is similar to [`futures::executor::block_on`].
    pub fn block_on<R>(&self, future: impl Future<Output = R>) -> R {
        futures::pin_mut!(future);

        let local_wake = Arc::new(Waker {
            apic: self.apic,
            processor_to_wake: self.apic.current_apic_id(),
            interrupt_vector: self.interrupt_vector.interrupt_num(),
            need_ipi: atomic::AtomicBool::new(false),
            woken_up: atomic::AtomicBool::new(false),
        });

        let waker = waker(local_wake.clone());
        let mut context = Context::from_waker(&waker);

        loop {
            if let Poll::Ready(val) = Future::poll(future.as_mut(), &mut context) {
                return val;
            }

            loop {
                debug_assert!(x86_64::instructions::interrupts::are_enabled());
                x86_64::instructions::interrupts::disable();

                // We store `true` in `need_ipi` before checking `woken_up`, otherwise there could be
                // a state where `need_ipi` is `false` but we've already checked `woken_up`.
                local_wake.need_ipi.store(true, atomic::Ordering::SeqCst);

                if local_wake
                    .woken_up
                    .compare_and_swap(true, false, atomic::Ordering::SeqCst)
                {
                    // We're going to poll the `Future` again, so `need_ipi` can be set to `false`.
                    local_wake.need_ipi.store(false, atomic::Ordering::SeqCst);
                    x86_64::instructions::interrupts::enable();
                    break;
                }

                // An `sti` opcode only takes effect after the *next* opcode, which is `hlt` here.
                // It is not possible for an interrupt to happen between `sti` and `hlt`.
                x86_64::instructions::interrupts::enable();
                x86_64::instructions::hlt();
            }
        }
    }
}

struct Waker {
    /// Reference to the APIC, for sending IPIs.
    apic: &'static LocalApicsControl,

    /// Identifier of the processor that this waker must wake up.
    processor_to_wake: ApicId,

    /// Which interrupt vector to send to the processor to wake it up.
    interrupt_vector: u8,

    /// Flag set to true if the processor has entered or has a chance to enter a halted state,
    /// and that an interprocess interrupt (IPI) is necessary in order to wake up the processor.
    ///
    /// If this is true, then you must set `woken_up` to true and send an IPI.
    /// If this is false, then setting `woken_up` to true is enough.
    need_ipi: atomic::AtomicBool,

    /// Flag to set to true in order to wake up the processor.
    woken_up: atomic::AtomicBool,
}

impl ArcWake for Waker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.woken_up.store(true, atomic::Ordering::SeqCst);

        if arc_self
            .need_ipi
            .compare_and_swap(true, false, atomic::Ordering::SeqCst)
        {
            if arc_self.processor_to_wake != arc_self.apic.current_apic_id() {
                arc_self.apic.send_interprocessor_interrupt(
                    arc_self.processor_to_wake,
                    arc_self.interrupt_vector,
                );
            }
        }
    }
}
