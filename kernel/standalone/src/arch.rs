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

//! Platform-specific code and kernel entry point.
//!
//! This module contains all the platform-specific code of the stand-alone kernel, plus the entry
//! point and initialization code.
//!
//! Initialization includes:
//!
//! - Initializing all CPU cores.
//! - Setting up a stack for each CPU core.
//! - Setting up the memory allocator in the [`mem_alloc`](crate::mem_alloc) module.
//! - Setting up a panic handler.
//!
//! After everything has been initialized, the entry point creates a struct that implements the
//! [`PlatformSpecific`] trait, and initializes and runs a [`Kernel`](crate::kernel::Kernel).

use core::{fmt, future::Future, num::NonZeroU32, pin::Pin};

mod arm;
mod riscv;
mod x86_64;

/// Access to all the platform-specific information.
// TODO: remove `'static` requirement
pub trait PlatformSpecific: Send + Sync + 'static {
    /// `Future` that fires when the monotonic clock reaches a certain value.
    // TODO: remove `'static` requirement
    type TimerFuture: Future<Output = ()> + Send + 'static;

    /// Returns the number of CPUs available.
    fn num_cpus(self: Pin<&Self>) -> NonZeroU32;

    /// Returns the number of nanoseconds that happened since an undeterminate moment in time.
    ///
    /// > **Note**: The returned value is provided on a "best effort" basis and is not
    /// >           necessarily exact (it is, in fact, rarely exact).
    fn monotonic_clock(self: Pin<&Self>) -> u128;
    /// Returns a `Future` that fires when the monotonic clock reaches the given value.
    fn timer(self: Pin<&Self>, clock_value: u128) -> Self::TimerFuture;

    /// Writes a `u8` on a port. Returns an error if the operation is not supported or if the port
    /// is out of range.
    unsafe fn write_port_u8(self: Pin<&Self>, port: u32, data: u8) -> Result<(), PortErr>;
    /// Writes a `u16` on a port. Returns an error if the operation is not supported or if the
    /// port is out of range.
    unsafe fn write_port_u16(self: Pin<&Self>, port: u32, data: u16) -> Result<(), PortErr>;
    /// Writes a `u32` on a port. Returns an error if the operation is not supported or if the
    /// port is out of range.
    unsafe fn write_port_u32(self: Pin<&Self>, port: u32, data: u32) -> Result<(), PortErr>;
    /// Reads a `u8` from a port. Returns an error if the operation is not supported or if the
    /// port is out of range.
    unsafe fn read_port_u8(self: Pin<&Self>, port: u32) -> Result<u8, PortErr>;
    /// Reads a `u16` from a port. Returns an error if the operation is not supported or if the
    /// port is out of range.
    unsafe fn read_port_u16(self: Pin<&Self>, port: u32) -> Result<u16, PortErr>;
    /// Reads a `u32` from a port. Returns an error if the operation is not supported or if the
    /// port is out of range.
    unsafe fn read_port_u32(self: Pin<&Self>, port: u32) -> Result<u32, PortErr>;
}

/// Error when requesting to read/write a hardware port.
#[derive(Debug)]
pub enum PortErr {
    /// Operation is not supported by the hardware.
    Unsupported,
    /// Port is out of range.
    OutOfRange,
}

impl fmt::Display for PortErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PortErr::Unsupported => write!(f, "Operation is not supported by the hardware"),
            PortErr::OutOfRange => write!(f, "Port is out of range"),
        }
    }
}
