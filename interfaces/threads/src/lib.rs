// Copyright(c) 2019 Pierre Krieger

//! Threads.

#![deny(intra_doc_link_resolution_failure)]

// TODO: everything here is a draft

use std::mem;

pub mod ffi;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn spawn_thread(function: impl FnOnce()) {
    let function_box: Box<Box<dyn FnOnce()>> = Box::new(Box::new(function));

    extern fn caller(user_data: u32) {
        unsafe {
            let user_data = Box::from_raw(user_data as *mut Box<dyn FnOnce()>);
            user_data();
        }
    }

    unsafe {
        let thread_new = ffi::ThreadsMessage::New(ffi::ThreadNew {
            fn_ptr: mem::transmute(caller as extern fn(u32)),
            user_data: Box::into_raw(function_box) as usize as u32,
        });

        syscalls::emit_message(&ffi::INTERFACE, &thread_new, false).unwrap();
    }
}
