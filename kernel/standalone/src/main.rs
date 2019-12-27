// Copyright (C) 2019  Pierre Krieger
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
#![feature(asm)]
#![feature(core_intrinsics)]
#![feature(panic_info_message)] // TODO: https://github.com/rust-lang/rust/issues/66745
#![feature(alloc_error_handler)] // TODO: https://github.com/rust-lang/rust/issues/66741
#![feature(naked_functions)] // TODO: https://github.com/rust-lang/rust/issues/32408
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))] // TODO: https://github.com/rust-lang/rust/issues/40180

extern crate alloc;
extern crate compiler_builtins;

mod arch;
mod hardware;
mod kernel;
mod mem_alloc;
mod panic;
mod random;

// This contains nothing. As the main entry point of the kernel is platform-specific, it is
// located in the `arch` module rather than here.
