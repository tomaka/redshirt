// Copyright(c) 2019 Pierre Krieger

//! Bindings for interfacing with the environment of the "kernel".

mod ffi;

pub fn register_interface() {
    unsafe { ffi::register_interface(); }
}
