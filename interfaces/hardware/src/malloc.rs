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

//! Allocation of physical memory.
//!
//! There are situations where it is necessary to pass to a device a pointer to a region of
//! memory. This is where this module comes into play.

use crate::{ffi, HardwareWriteOperationsBuilder, HardwareOperationsBuilder};

use alloc::{boxed::Box, vec, vec::Vec};
use core::{convert::TryFrom, marker::PhantomData, mem, ptr, slice};
use futures::prelude::*;

/// Buffer located in physical memory.
pub struct PhysicalBuffer<T: ?Sized> {
    /// Location of the buffer in physical memory.
    ptr: u64,
    /// Size in bytes of the buffer.
    size: u64,
    /// Marker to pin the `T` generic.
    marker: PhantomData<Box<T>>,
}

impl<T> PhysicalBuffer<T> {
    /// Creates a new buffer and moves `data` into it.
    ///
    /// > **Note**: `data` will **not** be free'd if you drop the buffer. In other words, it is as
    /// >           if you called `std::mem::forget(data)`. You should preferably not pass anything
    /// >           else than plain data, or call [`PhysicalBuffer::take`].
    ///
    pub fn new(data: T) -> impl Future<Output = Self> {
        let size = u64::try_from(mem::size_of_val(&data)).unwrap();
        let align = u64::try_from(mem::align_of_val(&data)).unwrap();

        malloc(size, align).map(move |ptr| {
            let buf = PhysicalBuffer {
                ptr,
                size,
                marker: PhantomData,
            };
            buf.write(data);
            buf
        })
    }

    /// Overwrites the content of the buffer with a new value.
    ///
    /// This moves `data` into the buffer. The previous value is **not** dropped, but simply
    /// leaked out.
    pub fn write(&self, data: T) {
        unsafe {
            let mut data_buf = Vec::<u8>::with_capacity(mem::size_of_val(&data));
            ptr::write_unaligned(data_buf.as_mut_ptr() as *mut T, data);
            data_buf.set_len(data_buf.capacity());

            let mut builder = HardwareWriteOperationsBuilder::with_capacity(1);
            builder.write(self.ptr, data_buf);
            builder.send();
        }
    }

    /// Reads back the content of the buffer and destroys the buffer.
    pub fn take(self) -> impl Future<Output = T> {
        unsafe { self.read_inner() }
    }

    /// Returns a copy of the content of the buffer.
    pub fn read(&self) -> impl Future<Output = T>
    where
        T: Copy,
    {
        unsafe { self.read_inner() }
    }

    /// Reads the content of the buffer and returns a copy.
    ///
    /// # Safety
    ///
    /// This function performs a copy of the content of the buffer. This is only safe if `T`
    /// implements `Copy`, or if you guarantee that no multiple copies of the same object are
    /// being read. In other words, this function is meant to be called from within
    /// [`PhysicalBuffer::take`] or [`PhysicalBuffer::read`].
    unsafe fn read_inner(&self) -> impl Future<Output = T> {
        // Note: we can't use `HardwareOperationsBuilder`, as this would require an `async`
        // function or block, which aren't available in `no_std` environments at the time of
        // writing.

        let msg =
            ffi::HardwareMessage::HardwareAccess(vec![ffi::Operation::PhysicalMemoryReadU8 {
                address: self.ptr,
                len: u32::try_from(mem::size_of::<T>()).unwrap(),
            }]);

        redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg)
            .unwrap()
            .map(move |mut response: Vec<ffi::HardwareAccessResponse>| {
                debug_assert_eq!(response.len(), 1);
                let buf = match response.remove(0) {
                    ffi::HardwareAccessResponse::PhysicalMemoryReadU8(val) => val,
                    _ => unreachable!(),
                };
                ptr::read_unaligned(buf.as_ptr() as *const T)
            })
    }
}

impl<T> PhysicalBuffer<[T]> {
    /// Allocates a new buffer with uninitialized contents.
    pub async fn new_uninit_slice(len: usize) -> PhysicalBuffer<[mem::MaybeUninit<T>]> {
        Self::new_uninit_slice_with_align(len, mem::align_of::<T>()).await
    }

    /// Allocates a new buffer with uninitialized contents.
    pub async fn new_uninit_slice_with_align(len: usize, align: usize) -> PhysicalBuffer<[mem::MaybeUninit<T>]> {
        let size = u64::try_from(mem::size_of::<T>()).unwrap().checked_mul(u64::try_from(len).unwrap()).unwrap();
        let align = u64::try_from(align).unwrap();

        PhysicalBuffer {
            ptr: malloc(size, align).await,
            size,
            marker: PhantomData,
        }
    }

    /// Returns the number of elements in the buffer.
    pub fn len(&self) -> usize {
        usize::try_from(self.size / u64::try_from(mem::size_of::<T>()).unwrap()).unwrap()
    }

    pub fn write_one(&self, idx: usize, value: T) {
        unsafe {
            if idx >= self.len() {
                panic!()
            }

            let mut ops = HardwareWriteOperationsBuilder::with_capacity(1);
            ops.write(
                self.ptr + u64::try_from(idx * mem::size_of::<T>()).unwrap(),
                slice::from_raw_parts(&value as *const T as *const u8, mem::size_of::<T>()).to_vec()
            );
            ops.send();

            mem::forget(value);
        }
    }
}

impl<T: Copy> PhysicalBuffer<[T]> {
    pub async fn read_one(&self, idx: usize) -> Option<T> {
        unsafe {
            if idx >= self.len() {
                return None;
            }

            let mut out = mem::MaybeUninit::<T>::uninit();

            let mut ops = HardwareOperationsBuilder::with_capacity(1);
            ops.read(
                self.ptr + u64::try_from(idx * mem::size_of::<T>()).unwrap(),
                slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, mem::size_of::<T>())
            );
            ops.send().await;

            Some(out.assume_init())
        }
    }

    pub async fn read_slice(&self, idx: usize, out: &mut [T]) {
        unsafe {
            assert!(idx + out.len() <= self.len());

            let mut ops = HardwareOperationsBuilder::with_capacity(1);
            ops.read(
                self.ptr + u64::try_from(idx * mem::size_of::<T>()).unwrap(),
                slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, out.len() * mem::size_of::<T>())
            );
            ops.send().await;
        }
    }

    pub fn write_slice(&self, idx: usize, data: &[T]) {
        unsafe {
            assert!(idx + data.len() <= self.len());

            let mut ops = HardwareWriteOperationsBuilder::with_capacity(1);
            ops.write(
                self.ptr + u64::try_from(idx * mem::size_of::<T>()).unwrap(),
                slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * mem::size_of::<T>()).to_vec()
            );
            ops.send();
        }
    }
}

impl<T> PhysicalBuffer<[mem::MaybeUninit<T>]> {
    /// Converts to an actual buffer.
    pub unsafe fn assume_init(self) -> PhysicalBuffer<[T]> {
        let size = self.size;
        let ptr = self.ptr;
        mem::forget(self);

        PhysicalBuffer {
            ptr,
            size,
            marker: PhantomData,
        }
    }
}

impl<T: ?Sized> PhysicalBuffer<T> {
    /// Returns the location in physical memory of the buffer.
    pub fn pointer(&self) -> u64 {
        self.ptr
    }
}

impl<T: ?Sized> Drop for PhysicalBuffer<T> {
    fn drop(&mut self) {
        free(self.ptr)
    }
}

/// Allocates physical memory.
///
/// # Panic
///
/// Panics if the allocation fails, for example if `size` is too large to be acceptable.
///
pub fn malloc(size: u64, alignment: u64) -> impl Future<Output = u64> {
    unsafe {
        let msg = ffi::HardwareMessage::Malloc { size, alignment };
        redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg)
            .unwrap()
            .map(move |ptr: u64| {
                assert_ne!(ptr, 0);
                debug_assert_eq!(ptr % alignment, 0);
                ptr
            })
    }
}

/// Frees physical memory previously allocated with [`malloc`].
pub fn free(ptr: u64) {
    unsafe {
        let msg = ffi::HardwareMessage::Free { ptr };
        redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &msg).unwrap();
    }
}
