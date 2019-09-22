// Copyright(c) 2019 Pierre Krieger

//! Bindings for interfacing with the environment of the "kernel".

#![deny(intra_doc_link_resolution_failure)]

use parity_scale_codec::{Encode as _};

pub mod ffi;

#[cfg(target_os = "unknown")]      // TODO: bad
pub fn register_interface(name: &str, f: extern fn() -> ()) {
    unsafe {
        let interface = ffi::Interface {
            name: name.to_owned(),
            fns: vec![
                ffi::InterfaceFn {
                    pointer: std::mem::transmute(f),        // TODO: make safer?
                    name: "bar".to_string(),
                }
            ],
        };

        let interface_bytes = interface.encode();
        ffi::register_interface(interface_bytes.as_ptr() as *const _, interface_bytes.len() as u32);
    }
}
