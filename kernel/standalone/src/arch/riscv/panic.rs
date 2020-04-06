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

//! Panic handling code.

use crate::klog::KLogger;

use alloc::sync::Arc;
use core::fmt::Write as _;
use redshirt_kernel_log_interface::ffi::{FramebufferFormat, FramebufferInfo, KernelLogMethod};
use spinning_top::Spinlock;

/// Modifies the logger to use when printing a panic.
pub fn set_logger(logger: KLogger) {
    *PANIC_LOGGER.lock() = Some(logger);
}

static PANIC_LOGGER: Spinlock<Option<KLogger>> = Spinlock::new(None);

#[cfg(not(any(test, doc, doctest)))]
#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    // TODO: somehow freeze all CPUs?

    let logger = PANIC_LOGGER.lock();

    // We only print a panic if the panic logger is set. This sucks, but there's no real way we
    // can handle panics before even basic initialization has been performed.
    if let Some(l) = &*logger {
        let mut printer = l.panic_printer();
        let _ = writeln!(printer, "Kernel panic!");
        let _ = writeln!(printer, "{}", panic_info);
        let _ = writeln!(printer, "");
    }

    // Freeze forever.
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
