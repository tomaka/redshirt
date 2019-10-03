// Copyright(c) 2019 Pierre Krieger

//! Threads.

#![deny(intra_doc_link_resolution_failure)]

use std::mem;

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

    syscalls::emit_message(&ffi::INTERFACE, &thread_new, false).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn spawn_thread(function: impl FnOnce()) {
    panic!()
}
