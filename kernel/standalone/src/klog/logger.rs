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
use spinning_top::{Spinlock, SpinlockGuard};

pub struct KLogger {
    inner: Spinlock<Inner>,
}

enum Inner {
    Disabled(KernelLogMethod),
    Enabled { terminal: Option<video::Terminal> },
}

impl KLogger {
    pub const unsafe fn new(method: KernelLogMethod) -> KLogger {
        if method.enabled {
            KLogger {
                inner: Spinlock::new(Inner::Enabled {
                    terminal: match method.framebuffer {
                        Some(fb) => Some(video::Terminal::new(fb)),
                        None => None,
                    },
                }),
            }
        } else {
            KLogger {
                inner: Spinlock::new(Inner::Disabled(method)),
            }
        }
    }

    /// Returns an object that implements `core::fmt::Write` for writing logs.
    ///
    /// The returned object holds a lock to some important information. Please call this method
    /// and destroy the object as soon as possible.
    pub fn log_printer<'a>(&'a self) -> impl fmt::Write + 'a {
        Printer {
            inner: self.inner.lock(),
            color: [0xdd, 0xdd, 0xdd],
        }
    }

    /// Returns an object that implements `core::fmt::Write` designed for printing a panic
    /// message.
    ///
    /// The returned object holds a lock to some important information. Please call this method
    /// and destroy the object as soon as possible.
    pub fn panic_printer<'a>(&'a self) -> impl fmt::Write + 'a {
        Printer {
            inner: self.inner.lock(),
            color: [0xff, 0x0, 0x0],
        }
    }

    /// Modifies the way logs should be printed.
    pub fn set_method(&self, method: KernelLogMethod) {
        unimplemented!() // TODO:
    }
}

struct Printer<'a> {
    inner: SpinlockGuard<'a, Inner>,
    color: [u8; 3],
}

impl<'a> fmt::Write for Printer<'a> {
    fn write_str(&mut self, message: &str) -> fmt::Result {
        match &mut *self.inner {
            Inner::Disabled(_) => {} // TODO: push to some buffer
            Inner::Enabled { terminal } => {
                if let Some(terminal) = terminal {
                    // TODO: red for panics
                    terminal.printer(self.color).write_str(message)?;
                }
            }
        }
        Ok(())
    }
}
