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
};
use core::{convert::TryFrom as _, mem, ptr, slice};

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
    apic: &Arc<ApicControl>,
    target: ApicId,
    boot_code: impl FnOnce() -> core::convert::Infallible + Send + 'static,
) {
    // TODO: clean up
    let layout = {
        let size = (_after_ap_start as *const u8 as usize)
            .checked_sub(_ap_start as *const u8 as usize)
            .unwrap();
        Layout::from_size_align(size, 0x1000).unwrap()
    };

    // Basic sanity check that the linker didn't somehow move our symbols around.
    debug_assert!(layout.size() < 1024);

    // FIXME: meh, do a proper allocation
    let bootstrap_code = 0x90000usize as *mut u8;/*{
        let buf = alloc::alloc(layout);
        assert!(!buf.is_null());
        buf
    };*/

    apic.send_interprocessor_init(target);
    let rdtsc = core::arch::x86_64::_rdtsc();

    ptr::copy_nonoverlapping(_ap_start as *const u8, bootstrap_code, layout.size());

    // We want the processor to call the `ap_after_boot` function defined below. This function
    // casts its first parameter into a `Box<Box<dyn FnOnce()>>` and calls it.
    // We cast `boot_code` into the proper format, then leak it with the intent to pass this
    // value to `ap_after_boot` (which will then "unleak" it).
    let ap_after_boot_param = {
        let boxed = Box::new(Box::new(boot_code) as Box<_>);
        let param_value: ApAfterBootParam = Box::into_raw(boxed);
        u64::try_from(param_value as usize).unwrap()
    };

    // The function `_ap_start64` starts with the following instructions:
    //
    // ```
    // 48 bc ef cd ab 90 78 56 34 12    movabs $0x1234567890abcdef, %rsp
    // 48 b8 ff ff 22 22 cc cc 99 99    movabs $0x9999cccc2222ffff, %rax
    // ```
    //
    // The values `0x1234567890abcdef` and `0x9999cccc2222ffff` are dummy values that are going
    // to overwrite in the block below.
    {
        let ap_start64_loc: *mut u8 = {
            let offset = (_ap_start64 as usize).checked_sub(_ap_start as usize).unwrap();
            bootstrap_code.add(offset)
        };

        // Perform some sanity check. Since we're performing dark magic, we really don't want to
        // do something wrong, or we will run into issues that are very hard to debug.
        assert_eq!(
            slice::from_raw_parts(ap_start64_loc as *const u8, 20),
            &[0x48, 0xbc, 0xef, 0xcd, 0xab, 0x90, 0x78, 0x56, 0x34, 0x12,
            0x48, 0xb8, 0xff, 0xff, 0x22, 0x22, 0xcc, 0xcc, 0x99, 0x99]
        );

        let stack_ptr_ptr = (ap_start64_loc.add(2)) as *mut u64;
        assert_eq!(stack_ptr_ptr.read_unaligned(), 0x1234567890abcdef);
        stack_ptr_ptr.write_unaligned(0x0); // TODO: stack pointer

        let param_ptr = (ap_start64_loc.add(12)) as *mut u64;
        assert_eq!(param_ptr.read_unaligned(), 0x9999cccc2222ffff);
        param_ptr.write_unaligned(ap_after_boot_param);
    }

    // Wait for 10ms to have elapsed since we sent the INIT IPI.
    super::executor::block_on(apic, apic.register_tsc_timer(rdtsc + 10_000_000));

    // Send the SINIT IPI, pointing to the bootstrap code that we have carefully crafted.
    apic.send_interprocessor_sipi(target, bootstrap_code as *const _);

    /*let rdtsc = unsafe { core::arch::x86_64::_rdtsc() };
    super::executor::block_on(apic, apic.register_tsc_timer(rdtsc + 1_000_000_000));
    apic.send_interprocessor_sipi(target, bootstrap_code as *const _);*/

    //alloc::dealloc(bootstrap_code, layout);
}

/// Actual type of the parameter passed to `ap_after_boot`.
type ApAfterBootParam = *mut Box<dyn FnOnce() -> core::convert::Infallible + Send + 'static>;

/// Called by `ap_boot.S` after set up.
///
/// When this function is called, the stack and paging have already been properly set up. The
/// first parameter is gathered from `rax` register according to the x86_64 calling convention.
#[no_mangle]
extern "C" fn ap_after_boot(to_exec: usize) -> ! {
    unsafe {
        let to_exec = to_exec as ApAfterBootParam;
        let to_exec = Box::from_raw(to_exec);
        let ret = (*to_exec)();
        match ret {} // TODO: remove this `ret` thingy once `!` is stable
    }
}

#[link(name = "apboot")]
extern "C" {
    fn _ap_start();
    fn _ap_start64();
    fn _after_ap_start();
}
