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

use crate::klog::video;

use core::{convert::TryFrom as _, fmt, hint};
use redshirt_kernel_log_interface::ffi::{KernelLogMethod, UartAccess, UartInfo};
use spinning_top::{guard::SpinlockGuard, Spinlock};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use x86_64::structures::port::{PortRead as _, PortWrite as _};

pub struct KLogger {
    inner: Spinlock<Inner>,
}

enum Inner {
    Disabled(KernelLogMethod),
    Enabled {
        terminal: Option<video::Terminal>,
        uart: Option<UartInfo>,
    },
}

impl KLogger {
    /// Initializes a disabled [`KLogger`].
    pub const fn disabled() -> KLogger {
        KLogger {
            inner: Spinlock::new(Inner::Disabled(KernelLogMethod {
                enabled: false,
                framebuffer: None,
                uart: None,
            })),
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
        // TODO: this isn't properly implemented ; on new terminal, which should re-print older logs
        unsafe {
            if method.enabled {
                *self.inner.lock() = Inner::Enabled {
                    terminal: match method.framebuffer {
                        Some(fb) => Some(video::Terminal::new(fb)),
                        None => None,
                    },
                    uart: method.uart,
                };
            } else {
                *self.inner.lock() = Inner::Disabled(method);
            }
        }
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
            Inner::Enabled { terminal, uart } => {
                if let Some(terminal) = terminal {
                    // TODO: red for panics
                    terminal.printer(self.color).write_str(message)?;
                }

                if let Some(uart) = uart {
                    unsafe {
                        for byte in message.as_bytes() {
                            loop {
                                let v = match uart.wait_address {
                                    UartAccess::MemoryMappedU32(r_addr) => {
                                        if let Ok(r_addr) = usize::try_from(r_addr) {
                                            (r_addr as *const u32).read_volatile()
                                        } else {
                                            0
                                        }
                                    }
                                    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                                    UartAccess::IoPortU8(r_port) => {
                                        u32::from(u8::read_from_port(r_port))
                                    }
                                    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
                                    UartAccess::IoPortU8(r_port) => 0,
                                };

                                if (v & uart.wait_mask) == uart.wait_compare_equal_if_ready {
                                    break;
                                }

                                hint::spin_loop();
                            }

                            match uart.write_address {
                                UartAccess::MemoryMappedU32(w_addr) => {
                                    if let Ok(w_addr) = usize::try_from(w_addr) {
                                        (w_addr as *mut u32).write_volatile(u32::from(*byte))
                                    }
                                }
                                #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                                UartAccess::IoPortU8(r_port) => {
                                    u8::write_to_port(r_port, *byte);
                                }
                                #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
                                UartAccess::IoPortU8(r_port) => {}
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
