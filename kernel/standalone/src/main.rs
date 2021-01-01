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

//! This program is meant to be invoked in a non-hosted environment. It never finishes.

#![no_std]
#![no_main]
#![feature(allocator_api)] // TODO: https://github.com/rust-lang/rust/issues/32838
#![feature(alloc_error_handler)] // TODO: https://github.com/rust-lang/rust/issues/66741
#![feature(asm)] // TODO: https://github.com/rust-lang/rust/issues/72016
#![feature(naked_functions)] // TODO: https://github.com/rust-lang/rust/issues/32408
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))] // TODO: https://github.com/rust-lang/rust/issues/40180

extern crate alloc;
extern crate rlibc; // TODO: necessary as a work-around for some linking issue; needs to be investigated

use alloc::sync::Arc;
use core::{pin::Pin, sync::atomic};

#[macro_use]
mod arch;

mod future_channel;
mod hardware;
mod kernel;
mod klog;
mod mem_alloc;
mod pci;
mod random;
mod time;

async fn main(platform_specific: Pin<Arc<arch::PlatformSpecific>>) -> ! {
    // Initialize the kernel once for all cores.
    static KERNEL: spinning_top::Spinlock<Option<Arc<kernel::Kernel>>> =
        spinning_top::Spinlock::new(None);

    // Initialize the kernel.
    // TODO: do this better than spinlocking, as initialization might be expensive
    let kernel = {
        let mut lock = KERNEL.lock();
        if let Some(existing_kernel) = lock.as_ref() {
            existing_kernel.clone()
        } else {
            let new_kernel = Arc::new(kernel::Kernel::init(platform_specific));
            *lock = Some(new_kernel.clone());
            new_kernel
        }
    };

    // Assign an index to each CPU.
    static CPU_INDEX: atomic::AtomicUsize = atomic::AtomicUsize::new(0);
    let cpu_index = CPU_INDEX.fetch_add(1, atomic::Ordering::Relaxed);

    // Run the kernel. This call never returns.
    kernel.run(cpu_index).await
}

__gen_boot! {
    entry: main,
    bss_start: __bss_start,
    bss_end: __bss_end,
}

extern "C" {
    static mut __bss_start: u8;
    static mut __bss_end: u8;
}
