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

//! Standalone redshirt kernel building tookit.
//!
//! This library provides a toolkit that lets one create a stand-alone redshirt kernel. Two things
//! are provided:
//!
//! - A `__gen_boot!` macro that gets passed a bunch of information about the target platform, and
//! generates a function exported under the symbol `_start`.
//! - A `run` function, suitable to be passed to the `gen_boot!` macro, that runs the kernel after
//! an environment has been setup.
//!
//! It is intended that in the future this crate allows more customizations, and a more
//! fine-grained split of components.
//!
//! # Kernel environment
//!
//! When the `_gen_boot!` macro is used, a symbol named `_start` is generated. The user is
//! responsible for ensuring that execution jumps to this symbol, after which the code of the
//! macro is in total control of the hardware.
//!
//! No assumption is made about the state of the registers, memory, or hardware when `_start` is
//! executed.
//!
//! The only exception concerns the x86 and x86_64 platform, where `_start` is expected to be
//! loaded from a multiboot2-compatible loader.
//! See <https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html> for more information.
//!
//! Additionally, this crate defines [a panic handler](https://doc.rust-lang.org/reference/runtime.html#the-panic_handler-attribute)
//! and a [global allocator](https://doc.rust-lang.org/reference/runtime.html#the-global_allocator-attribute).
//! It is not possible to set your own panic handler or global allocator when having this crate
//! as a dependency.

#![no_std]
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
pub mod arch;

mod future_channel;
mod hardware;
mod pci;
mod random;
mod time;

// TODO: don't make public
#[doc(hidden)]
pub mod klog;
#[doc(hidden)]
pub mod mem_alloc;

// Re-exports necessary to make the `__gen_boot!` macro work.
// TODO: don't make public
#[doc(hidden)]
pub extern crate futures;
#[doc(hidden)]
pub extern crate redshirt_kernel_log_interface;

// TODO: instead of having a public `kernel` module, this library should instead expose the various components, and the user builds the kernel themselves
pub mod kernel;

pub async fn run(platform_specific: Pin<Arc<arch::PlatformSpecific>>) -> ! {
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
