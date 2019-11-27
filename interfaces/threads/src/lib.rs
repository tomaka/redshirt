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

//! Threads.

#![deny(intra_doc_link_resolution_failure)]
#![no_std]

use core::mem;

pub mod ffi;

///
/// > **WARNING**: DON'T USE THIS FUNCTION.
///
/// > **WARNING**: Rust (and more importantly LLVM) at the moment assumes that only a single WASM
/// >              thread can exist at any given point in time. More specifically, LLVM assumes
/// >              that only a single stack exists, and maintains a stack pointer as a global
/// >              variable. It is therefore unsound to use stack variables on separate threads.
#[cfg(target_arch = "wasm32")]
pub unsafe fn spawn_thread(function: impl FnOnce()) {
    let function_box: Box<Box<dyn FnOnce()>> = Box::new(Box::new(function));

    extern "C" fn caller(user_data: u32) {
        unsafe {
            let user_data = Box::from_raw(user_data as *mut Box<dyn FnOnce()>);
            user_data();
        }
    }

    let thread_new = ffi::ThreadsMessage::New(ffi::ThreadNew {
        fn_ptr: mem::transmute(caller as extern "C" fn(u32)),
        user_data: Box::into_raw(function_box) as usize as u32,
    });

    nametbd_syscalls_interface::emit_message(&ffi::INTERFACE, &thread_new, false).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn spawn_thread(function: impl FnOnce()) {
    panic!()
}
