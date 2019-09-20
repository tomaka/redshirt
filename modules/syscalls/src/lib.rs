// Copyright(c) 2019 Pierre Krieger

//! Bindings for interfacing with the environment of the "kernel".

use parity_scale_codec::{Encode as _};

pub mod ffi;

#[cfg(feature = "static-link")]
pub fn register_interface(name: &str) {
    unsafe {
        let interface = ffi::Interface {
            name: name.to_owned(),
            fns: vec![
                ffi::InterfaceFn {
                    pointer: 12,
                    params: Vec::new(),
                }
            ],
        };

        let interface_bytes = interface.encode();
        ffi::register_interface(interface_bytes.as_ptr() as *const _, interface_bytes.len() as u32);
    }
}
