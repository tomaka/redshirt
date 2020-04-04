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

use crate::klog::video;

use core::fmt;
use redshirt_kernel_log_interface::ffi::{FramebufferFormat, KernelLogMethod};
use spinning_top::Spinlock;

pub struct KLogger {
    inner: Spinlock<Inner>,
}

struct Inner {
    method: KernelLogMethod,
    terminal: Option<video::Terminal>,
}

impl KLogger {
    pub const fn new(method: KernelLogMethod) -> KLogger {
        KLogger {
            inner: Spinlock::new(Inner {
                method,
                terminal: None,
            }),
            method: Spinlock::new(method),
        }
    }

    /// Returns an object that implements `core::fmt::Write` for writing logs.
    pub fn log_printer<'a>(&'a self) -> impl fmt::Write + 'a {
        Printer {
            klogger: self,
            panic_message: false,
        }
    }

    /// Returns an object that implements `core::fmt::Write` designed for printing a panic
    /// message.
    pub fn panic_printer<'a>(&'a self) -> impl fmt::Write + 'a {
        Printer {
            klogger: self,
            panic_message: true,
        }
    }

    /// Modifies the way logs should be printed.
    pub fn set_method(&self, method: KernelLogMethod) {
        *self.method.lock() = method;
    }
}

struct Printer<'a> {
    klogger: &'a KLogger,
    panic_message: bool,
}
