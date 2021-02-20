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

use core::fmt::{self, Write as _};
use redshirt_kernel_log_interface::ffi::KernelLogMethod;

pub static PANIC_LOGGER: KLogger = KLogger::disabled();

#[cfg(not(any(test, doc, doctest)))]
#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    let mut printer = PANIC_LOGGER.panic_printer();
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
