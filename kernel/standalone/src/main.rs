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
#![feature(core_intrinsics)]
#![feature(const_if_match)] // TODO: https://github.com/rust-lang/rust/issues/49146
#![feature(global_asm)] // TODO: https://github.com/rust-lang/rust/issues/35119
#![feature(llvm_asm)] // TODO: replace all occurrences of `llvm_asm!` with `asm!`
#![feature(naked_functions)] // TODO: https://github.com/rust-lang/rust/issues/32408
#![feature(panic_info_message)] // TODO: https://github.com/rust-lang/rust/issues/66745
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))] // TODO: https://github.com/rust-lang/rust/issues/40180

extern crate alloc;
extern crate rlibc; // TODO: necessary as a work-around for some linking issue; needs to be investigated

mod arch;
mod hardware;
mod kernel;
mod klog;
mod mem_alloc;
mod pci;
mod random;
mod time;

// This contains nothing. As the main entry point of the kernel is platform-specific, it is
// located in the `arch` module rather than here.
