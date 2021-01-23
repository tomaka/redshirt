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

//! Panic handling code.

use crate::klog::KLogger;

use alloc::sync::Arc;
use core::fmt::Write as _;
use spinning_top::Spinlock;

/// Modifies the logger to use when printing a panic.
pub fn set_logger(logger: Arc<KLogger>) {
    *PANIC_LOGGER.lock() = Some(logger);
}

static PANIC_LOGGER: Spinlock<Option<Arc<KLogger>>> = Spinlock::new(None);
static DEFAULT_LOGGER: KLogger = unsafe { KLogger::new(super::DEFAULT_LOG_METHOD) };

#[cfg(not(any(test, doc, doctest)))]
#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    let logger = PANIC_LOGGER.lock();
    let logger = if let Some(l) = &*logger {
        l
    } else {
        &DEFAULT_LOGGER
    };

    let mut printer = logger.panic_printer();
    let _ = writeln!(printer, "Kernel panic!");
    let _ = writeln!(printer, "{}", panic_info);
    let _ = writeln!(printer, "");
    drop(printer);

    // Freeze forever.
    loop {
        x86_64::instructions::interrupts::disable();
        x86_64::instructions::hlt();
    }
}
