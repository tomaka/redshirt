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

use crate::arch::x86_64::apic::{ApicControl, ApicId};
use ::alloc::{
    alloc::{self, Layout},
    sync::Arc,
};
use core::ptr;

///
/// # Safety
///
/// This function must only be called once per `target`.
/// The `target` must not be the local processor.
///
pub unsafe fn boot_associated_processor(
    apic: &Arc<ApicControl>,
    target: ApicId,
    boot_code: impl FnOnce() + Send,
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

    // FIXME: meh
    let bootstrap_code = 0x90000usize as *mut u8;/*{
        let buf = alloc::alloc(layout);
        assert!(!buf.is_null());
        buf
    };*/

    // apic.send_interprocessor_init(target); TODO: ???

    ptr::copy_nonoverlapping(_ap_start as *const u8, bootstrap_code, layout.size());

    apic.send_interprocessor_init(target);
    apic.send_interprocessor_sipi(target, bootstrap_code as *const _);

    /*let rdtsc = unsafe { core::arch::x86_64::_rdtsc() };
    super::executor::block_on(apic, apic.register_tsc_timer(rdtsc + 1_000_000_000));
    apic.send_interprocessor_sipi(target, bootstrap_code as *const _);*/

    //alloc::dealloc(bootstrap_code, layout);
}

#[link(name = "apboot")]
extern "C" {
    fn _ap_start();
    fn _after_ap_start();
}
