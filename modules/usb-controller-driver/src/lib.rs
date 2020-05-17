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

// TODO: all this code should be moved to a separate repo

//! USB host controller driver and devices manager.
//!
//! This library is meant to be used in the context of operating system development and works on
//! bare metal.
//!
//! This library allows interacting with USB host controllers and interacting with the USB devices
//! that are connected to them.
//!
//! # What you need
//!
//! - You must implement the [`HwAccessRef`] trait, which allows the library to communicate with
//! the physical memory. This implementation must be aware of concerns regarding virtual memory
//! and caching. See the documentation of the trait for more detail.
//!
//! - The primary way USB host controllers interact with the operating system is through
//! memory-mapped registers. However, be aware that finding the location of these memory-mapped
//! registers is out of scope of this library. On x86 platforms, a USB host controller is normally
//! a PCI device and can be found by enumerating PCI devices.
//!
//! - Detecting interrupts is also out of scope of this library. When a USB host controller fires
//! an interrupt, you must call [`Usb::on_interrupt`] as soon as possible in response. This
//! function checks the state of the controllers to determine what has happened since the last
//! time it has been called. There is no drawback in calling [`Usb::on_interrupt`] even if no
//! interrupt has actually been triggered (except for the performance cost of calling the
//! function), so you don't need to worry too much about interrupts colliding. If multiple
//! interrupts happen before you had the chance to call this function, you only have to call it
//! once.
//!
//! # Usage
//!
//! Create a [`Usb`] state machine, passing an implementation of [`HwAccessRef`]. This state
//! machine will have exclusive ownership of all the host controllers and all the USB devices.
//! It manages sending and receiving packets, enabling/disabling devices, assigning addresses,
//! hubs, and so on.
//!
//! Finding where USB host controllers are mapped in memory if out of scope of this library. Call
//! [`Usb::add_ohci`] once you have found an OHCI controller.
//!
//! > **Note**: At the time of this writing, only OHCI (one of the two USB 1.1 controllers
//!             standards) is supported.
//!

#![no_std]

// TODO: change everything to accept an `AllocRef` trait implementation, instead of doing implicit
// allocations
extern crate alloc;

use core::{
    alloc::Layout,
    future::Future,
    num::{NonZeroU32, NonZeroU64},
    time::Duration,
};

mod control_packets;
mod devices;
mod ohci;
mod usb;

pub use ohci::InitError;
pub use usb::Usb;

/// Abstraction over the hardware.
///
/// The code of this library doesn't assume that it can directly access physical memory. Instead,
/// any access to physical memory is done through this trait.
///
/// This trait is designed to be implemented on references, and not on plain types. For instance,
/// if you define a type `Foo`, you are encouraged to implement this trait on `&Foo`.
///
/// This trait is used in order to write data in memory that a USB controller will later read, or
/// to read data from memory that a USB controller has written. As such, reads and writes should
/// bypass processor caches. Pointers returned by [`HwAccessRef::alloc32`] and
/// [`HwAccessRef::alloc64`] will be passed to USB controllers and must therefore refer to actual
/// physical memory addresses.
///
/// # Safety
///
/// Code that uses this trait relies on the fact that the various methods are implemented in a
/// correct way. For example, allocating multiple buffers must not yield overlapping buffers.
///
pub unsafe trait HwAccessRef<'a>: Copy + Clone {
    type Delay: Future<Output = ()> + 'a;
    type ReadMemFutureU8: Future<Output = ()> + 'a;
    type ReadMemFutureU32: Future<Output = ()> + 'a;
    type WriteMemFutureU8: Future<Output = ()> + 'a;
    type WriteMemFutureU32: Future<Output = ()> + 'a;
    // TODO: the error type should be core::alloc::AllocErr once it's stable
    type Alloc64: Future<Output = Result<NonZeroU64, ()>> + 'a;
    // TODO: the error type should be core::alloc::AllocErr once it's stable
    type Alloc32: Future<Output = Result<NonZeroU32, ()>> + 'a;

    /// Performs a serie of atomic physical memory reads starting at the given address.
    unsafe fn read_memory_u8(self, address: u64, dest: &'a mut [u8]) -> Self::ReadMemFutureU8;

    /// Performs a serie of atomic physical memory reads starting at the given address.
    ///
    /// The data must be read in little endian. If the current platform is big endian, you should
    /// call `swap_bytes` beforehand.
    ///
    /// `address` must be a multiple of 4.
    unsafe fn read_memory_u32_le(self, address: u64, dest: &'a mut [u32])
        -> Self::ReadMemFutureU32;

    /// Performs a serie of atomic physical memory writes starting at the given address.
    unsafe fn write_memory_u8(self, address: u64, data: &[u8]) -> Self::WriteMemFutureU8;

    /// Performs a serie of atomic physical memory writes starting at the given address.
    ///
    /// The data must be written in little endian. If the current platform is big endian, you
    /// should call `swap_bytes` beforehand.
    ///
    /// `address` must be a multiple of 4.
    unsafe fn write_memory_u32_le(self, address: u64, data: &[u32]) -> Self::WriteMemFutureU32;

    /// Allocate a memory buffer in physical memory. Does not need to be cleared with 0s.
    ///
    /// The returned pointer will likely be passed to the USB controller and read by the USB
    /// controller.
    ///
    /// > **Note**: The value returned is a `u64` and not a pointer, as the buffer is not
    /// >           necessarily directly accessible. All accesses to the buffer must be performed
    /// >           through the other methods of this trait.
    fn alloc64(self, layout: Layout) -> Self::Alloc64;

    /// Same as [`HwAccessRef::alloc64`], except that the returned buffer must fit within the
    /// first four gigabytes of physical memory.
    // TODO: is this distinction with alloc64? I did it because USB 1 only allows 32bits addresses
    //       while I suspect that USB 3 accepts 64bits addresses
    fn alloc32(self, layout: Layout) -> Self::Alloc32;

    /// Deallocates a previously-allocated block of physical memory.
    ///
    /// If `alloc32` is true, then this buffer was allocated using [`HwAccessRef::alloc32`].
    ///
    /// # Safety
    ///
    /// `address` must be a value previously-returned by a call to `alloc`, and `layout` must
    /// match the layout that was passed to `alloc`.
    unsafe fn dealloc(self, address: u64, alloc32: bool, layout: Layout);

    /// Returns a future that is ready after the given duration has passed.
    fn delay(self, duration: Duration) -> Self::Delay;
}

/// Status of a port of a hub.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
// TODO: move somewhere?
pub enum PortState {
    /// No electricity on the port.
    NotPowered,
    /// Port is powered but no device is connected. If a device connects, the port switches to
    /// `Disabled`.
    Disconnected,
    /// Port is connected to a device, but disabled. No data transferts are possible. Must be
    /// followed by a reset of the port.
    Disabled,
    /// Port is connected to a device and is currently emitting a "reset" signal to the device
    /// for the device to reset its state. Normally followed with `Enabled`.
    Resetting,
    /// Normal state. Connected to a device. Transferts go through.
    Enabled,
    /// Port is normally enabled but temporarily disabled. No data transferts are possible. This
    /// state can only be entered if asking the hub to suspend a port. Neither devices or the hub
    /// itself can ask for a port to be suspended.
    Suspended,
    /// Port was suspended and is currently emitting a "resume" signal. Will be followed by
    /// `Enabled` if everything goes well.
    Resuming,
}

/// Type of data transfers supported by an endpoint. Each endpoint only supports one kind of data
/// transfers.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum EndpointTy {
    Control,
    Bulk,
    Interrupt,
    Isochronous,
}

// TODO: move to different module
pub struct Buffer32<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    hardware_access: TAcc,
    buffer: NonZeroU32,
    layout: Layout,
}

impl<TAcc> Buffer32<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    pub async fn new(hardware_access: TAcc, layout: Layout) -> Buffer32<TAcc> {
        let buffer = match hardware_access.alloc32(layout).await {
            Ok(b) => b,
            Err(_) => alloc::alloc::handle_alloc_error(layout), // TODO: return error instead
        };

        Buffer32 {
            hardware_access,
            buffer,
            layout,
        }
    }

    /// Returns the physical memory address of the buffer.
    ///
    /// This value never changes and is valid until the [`Buffer32`] is destroyed.
    pub fn pointer(&self) -> NonZeroU32 {
        self.buffer
    }
}

impl<TAcc> Drop for Buffer32<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    fn drop(&mut self) {
        unsafe {
            self.hardware_access
                .dealloc(u64::from(self.buffer.get()), true, self.layout);
        }
    }
}
