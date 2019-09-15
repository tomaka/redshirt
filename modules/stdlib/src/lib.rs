// Copyright(c) 2019 Pierre Krieger

//! Bindings for interfacing with the environment of the "kernel".

mod ffi;

pub fn register_interface(name: &str) {
    unsafe {
        let interface = ffi::Interface {
            name: name.as_bytes().as_ptr() as *const _,
            name_len: name.as_bytes().len(),
            // TODO: obviously wrong
            fns: core::ptr::null_mut(),
            fns_len: 0,
        };

        ffi::register_interface(&interface);
    }
}
