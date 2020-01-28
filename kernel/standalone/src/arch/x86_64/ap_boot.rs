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

//! Bootstrapping associated processors.
//!
//! On x86 and x86_64 platforms, processors are divided in two categories: one BSP (bootstrap
//! processor) and zero or more APs (associated processors). Only the BSP initially starts,
//! and the APs have to be manually started from the BSP. This what this module is responsible
//! for doing.
//!
//! # Usage
//!
//! // TODO: write up
//!

use crate::arch::x86_64::apic::{ApicControl, ApicId};
use ::alloc::{
    alloc::{self, Layout},
    boxed::Box,
    sync::Arc,
    vec::Vec,
};
use core::{convert::TryFrom as _, mem, ops::Range, ptr, slice};
use futures::channel::oneshot;

pub struct ApBootAlloc {
    inner: linked_list_allocator::Heap,
}

pub fn filter_build_ap_boot_alloc<'a>(
    ranges: impl Iterator<Item = Range<usize>> + 'a,
    alloc: &'a mut Option<ApBootAlloc>,
) -> impl Iterator<Item = Range<usize>> + 'a {
    const WANTED: usize = 0x4000;

    ranges.filter_map(move |mut range| {
        if alloc.is_none() {
            // FIXME: this is a hack because in practice our RAM starts at 0.
            // FIXME: this should probably instead be fixed in the linked_list_allocator
            if range.start == 0 {
                range.start = 1;
            }

            let range_size = range.end - range.start;
            if range.start + WANTED <= 0x100000 && range_size >= WANTED {
                *alloc = Some(ApBootAlloc {
                    inner: unsafe { linked_list_allocator::Heap::new(range.start, WANTED) },
                });
                if range_size > WANTED {
                    return Some((range.start + WANTED..range.end));
                }
                return None;
            }
        }

        Some(range)
    })
}

/// Bootstraps the given processor, making it execute `boot_code`.
///
/// # Safety
///
/// This function must only be called once per `target`.
/// The `target` must not be the local processor.
///
// TODO: replace `Infallible` with `!` when stable
#[cold]
pub unsafe fn boot_associated_processor(
    alloc: &mut ApBootAlloc,
    apic: &Arc<ApicControl>,
    target: ApicId,
    boot_code: impl FnOnce() -> core::convert::Infallible + Send + 'static,
) {
    // In order to boot an associated processor, we must send to it an inter-processor interrupt
    // (IPI) containing the offset of a 4kiB memory page containing the code that it must start
    // executing. The CS register of the target processor will take the value that we send, the
    // IP register will be set to 0, and the processor will start running in 16 bits mode.
    //
    // Since this is 16 bits mode, the processor cannot execute any code (or access any data)
    // above one megabyte of physical memory. Most, if not all, of the kernel is loaded above
    // that limit, and therefore we cannot simply ask the processor to start executing a certain
    // function.
    //
    // Instead, what we must do is allocate a buffer below that one megabyte limit, write some
    // x86 machine code in that buffer, and then we can ask the processor to run it. This is
    // implemented by copying a template code into that buffer and tweaking the constants.
    //
    // The template in question is located in the `ap_boot.S` file. See also the documentation
    // there.

    // We start by allocating the buffer where to write the bootstrap code.
    let mut bootstrap_code_buf = {
        let size = (_ap_boot_end as *const u8 as usize)
            .checked_sub(_ap_boot_start as *const u8 as usize)
            .unwrap();
        // Basic sanity check to make sure that nothing's fundamentally wrong.
        assert!(size <= 0x1000);
        let layout = Layout::from_size_align(size, 0x1000).unwrap();
        Allocation::new(&mut alloc.inner, layout)
    };

    apic.send_interprocessor_init(target);
    let rdtsc = core::arch::x86_64::_rdtsc();

    ptr::copy_nonoverlapping(
        _ap_boot_start as *const u8,
        bootstrap_code_buf.as_mut_ptr(),
        bootstrap_code_buf.size(),
    );

    // Later, we will want to wait until the AP has finished initializing. To do so, we create
    // a channel and modify `boot_code` to signal that channel before doing anything more.
    let (boot_code, init_finished_future) = {
        let (tx, rx) = oneshot::channel();
        let boot_code = move || {
            let _ = tx.send(());
            boot_code()
        };
        (boot_code, rx)
    };

    // We want the processor we bootstrap to call the `ap_after_boot` function defined below. This
    // function will cast its first parameter into a `Box<Box<dyn FnOnce()>>` and call it.
    // We therefore cast `boot_code` into the proper format, then leak it with the intent to pass
    // this value to `ap_after_boot` (which will then "unleak" it).
    let ap_after_boot_param = {
        let boxed = Box::new(Box::new(boot_code) as Box<_>);
        let param_value: ApAfterBootParam = Box::into_raw(boxed);
        u64::try_from(param_value as usize).unwrap()
    };

    // Allocate a stack for the processor. This is the one and unique stack that will be used for
    // everything.
    let stack_size = 10 * 1024 * 1024usize;
    let stack_top = {
        let layout = Layout::from_size_align(stack_size, 0x1000).unwrap();
        let ptr = alloc::alloc(layout);
        assert!(!ptr.is_null());
        u64::try_from(ptr as usize + stack_size).unwrap()
    };

    // There exists several markers in the template that we must adjust before starting it.
    //
    // The code at symbol `_ap_boot_marker1` starts with the following instructions:
    //
    // ```
    // 66 ea ad de ad de 08    ljmpl  $0x8, $0xdeaddead
    // ```
    //
    // The code at symbol `_ap_boot_marker3` starts with the following instructions:
    //
    // ```
    // 66 ba dd ba 00 ff    mov $0xff00badd, %edx
    // ```
    //
    // The code at symbol `_ap_boot_marker2` starts with the following instructions:
    //
    // ```
    // 48 bc ef cd ab 90 78 56 34 12    movabs $0x1234567890abcdef, %rsp
    // 48 b8 ff ff 22 22 cc cc 99 99    movabs $0x9999cccc2222ffff, %rax
    // ```
    //
    // The values `0xdeaddead`, `0xff00badd`, `0x1234567890abcdef`, and `0x9999cccc2222ffff` are
    // dummy values that we overwrite in the block below.
    {
        let ap_boot_marker1_loc: *mut u8 = {
            let offset = (_ap_boot_marker1 as usize)
                .checked_sub(_ap_boot_start as usize)
                .unwrap();
            bootstrap_code_buf.as_mut_ptr().add(offset)
        };
        let ap_boot_marker2_loc: *mut u8 = {
            let offset = (_ap_boot_marker2 as usize)
                .checked_sub(_ap_boot_start as usize)
                .unwrap();
            bootstrap_code_buf.as_mut_ptr().add(offset)
        };
        let ap_boot_marker3_loc: *mut u8 = {
            let offset = (_ap_boot_marker3 as usize)
                .checked_sub(_ap_boot_start as usize)
                .unwrap();
            bootstrap_code_buf.as_mut_ptr().add(offset)
        };

        // Perform some sanity check. Since we're performing dark magic, we really want to be sure
        // that we're overwriting the correct code, or we will run into issues that are very hard
        // to debug.
        assert_eq!(
            slice::from_raw_parts(ap_boot_marker1_loc as *const u8, 7),
            &[0x66, 0xea, 0xad, 0xde, 0xad, 0xde, 0x08]
        );
        assert_eq!(
            slice::from_raw_parts(ap_boot_marker2_loc as *const u8, 20),
            &[
                0x48, 0xbc, 0xef, 0xcd, 0xab, 0x90, 0x78, 0x56, 0x34, 0x12, 0x48, 0xb8, 0xff, 0xff,
                0x22, 0x22, 0xcc, 0xcc, 0x99, 0x99
            ]
        );
        assert_eq!(
            slice::from_raw_parts(ap_boot_marker3_loc as *const u8, 6),
            &[0x66, 0xba, 0xdd, 0xba, 0x00, 0xff]
        );

        let stack_ptr_ptr = (ap_boot_marker2_loc.add(2)) as *mut u64;
        assert_eq!(stack_ptr_ptr.read_unaligned(), 0x1234567890abcdef);
        stack_ptr_ptr.write_unaligned(stack_top);

        let param_ptr = (ap_boot_marker2_loc.add(12)) as *mut u64;
        assert_eq!(param_ptr.read_unaligned(), 0x9999cccc2222ffff);
        param_ptr.write_unaligned(ap_after_boot_param);

        let ljmp_target = u32::try_from(ap_boot_marker2_loc as usize).unwrap();
        let ljmp_target_ptr = (ap_boot_marker1_loc.add(2)) as *mut u32;
        assert_eq!(ljmp_target_ptr.read_unaligned(), 0xdeaddead);
        ljmp_target_ptr.write_unaligned(ljmp_target);

        // Read the value from the CR3 register.
        let pml_addr = x86_64::registers::control::Cr3::read()
            .0
            .start_address()
            .as_u64();
        let pml_addr = u32::try_from(pml_addr).unwrap();
        let pml_addr_ptr = (ap_boot_marker3_loc.add(2)) as *mut u32;
        assert_eq!(pml_addr_ptr.read_unaligned(), 0xff00badd);
        pml_addr_ptr.write_unaligned(pml_addr);
    }

    // Wait for 10ms to have elapsed since we sent the INIT IPI.
    super::executor::block_on(apic, apic.register_tsc_timer(rdtsc + 10_000_000));

    // Send the SINIT IPI, pointing to the bootstrap code that we have carefully crafted.
    apic.send_interprocessor_sipi(target, bootstrap_code_buf.as_mut_ptr() as *const _);

    /*let rdtsc = unsafe { core::arch::x86_64::_rdtsc() };
    super::executor::block_on(apic, apic.register_tsc_timer(rdtsc + 1_000_000_000));
    apic.send_interprocessor_sipi(target, bootstrap_code_buf.as_mut_ptr() as *const _);*/

    // Wait for CPU initialization to finish.
    // TODO: doesn't work; requires the child CPU to set up their APIC
    //super::executor::block_on(&apic, init_finished_future).unwrap();

    // Make sure the buffer is dropped at the end.
    // TODO: drop(bootstrap_code_buf);
    mem::forget(bootstrap_code_buf);
}

/// There is surprisingly no type in the Rust standard library that keeps track of an allocation.
struct Allocation<'a, T: alloc::Alloc> {
    alloc: &'a mut T,
    inner: ptr::NonNull<u8>,
    layout: Layout,
}

impl<'a, T: alloc::Alloc> Allocation<'a, T> {
    fn new(alloc: &'a mut T, layout: Layout) -> Self {
        unsafe {
            let buf = alloc.alloc(layout).unwrap();
            Allocation {
                alloc,
                inner: buf,
                layout,
            }
        }
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.inner.as_ptr()
    }

    fn size(&self) -> usize {
        self.layout.size()
    }
}

impl<'a, T: alloc::Alloc> Drop for Allocation<'a, T> {
    fn drop(&mut self) {
        unsafe { self.alloc.dealloc(self.inner, self.layout) }
    }
}

/// Actual type of the parameter passed to `ap_after_boot`.
type ApAfterBootParam = *mut Box<dyn FnOnce() -> core::convert::Infallible + Send + 'static>;

/// Called by `ap_boot.S` after setup.
///
/// When this function is called, the stack and paging have already been properly set up. The
/// first parameter is gathered from the `rdi` register according to the x86_64 calling
/// convention.
#[no_mangle]
extern "C" fn ap_after_boot(to_exec: usize) -> ! {
    unsafe {
        let to_exec = to_exec as ApAfterBootParam;
        let to_exec = Box::from_raw(to_exec);
        let ret = (*to_exec)();
        match ret {} // TODO: remove this `ret` thingy once `!` is stable
    }
}

/// See the documentation in `ap_boot.S`.
#[link(name = "apboot")]
extern "C" {
    fn _ap_boot_start();
    fn _ap_boot_marker1();
    fn _ap_boot_marker2();
    fn _ap_boot_marker3();
    fn _ap_boot_end();
}
