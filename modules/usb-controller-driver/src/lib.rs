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

pub mod ohci; // TODO: private

/// Abstraction over the hardware.
///
/// The code of this library doesn't assume that it can directly access physical memory. Instead,
/// any access to physical memory is done through this trait.
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
    /// The data must be read in big endian. If the current platform is little endian (which is
    /// the case for x86), you should call `swap_bytes` after the read.
    ///
    /// `address` must be a multiple of 4.
    unsafe fn read_memory_u32_be(self, address: u64, dest: &'a mut [u32])
        -> Self::ReadMemFutureU32;
    /// Performs a serie of atomic physical memory writes starting at the given address.
    unsafe fn write_memory_u8(self, address: u64, data: &[u8]) -> Self::WriteMemFutureU8;
    /// Performs a serie of atomic physical memory writes starting at the given address.
    ///
    /// The data must be written in big endian. If the current platform is little endian (which is
    /// the case for x86), you should call `swap_bytes` beforehand.
    ///
    /// `address` must be a multiple of 4.
    unsafe fn write_memory_u32_be(self, address: u64, data: &[u32]) -> Self::WriteMemFutureU32;

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
