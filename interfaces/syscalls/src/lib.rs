// Copyright(c) 2019 Pierre Krieger

//! Bindings for interfacing with the environment of the "kernel".

#![deny(intra_doc_link_resolution_failure)]

use parity_scale_codec::{Encode as _};

pub mod ffi;

#[cfg(target_os = "unknown")]      // TODO: bad
pub fn register_interface(hash: [u8; 32]) -> Result<(), ()> {
    unsafe {
        let ret = ffi::register_interface(&hash as *const [u8; 32] as *const _);
        if ret == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}
