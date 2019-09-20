// Copyright(c) 2019 Pierre Krieger

use core::ffi::c_void;
use parity_scale_codec::{Encode, Decode};

#[cfg(feature = "static-link")]
#[link(wasm_import_module = "")]
extern "C" {
    pub(crate) fn register_interface(interface: *const c_void, interface_len: u32) -> i32;
}

#[derive(Debug, Encode, Decode)]
pub struct Interface {
    pub name: String,
    pub fns: Vec<InterfaceFn>,
}

#[derive(Debug, Encode, Decode)]
pub struct InterfaceFn {
    pub pointer: i32,
    pub params: Vec<InterfaceParam>,
}

#[derive(Debug, Encode, Decode)]
pub struct InterfaceParam {
    pub ty: InterfaceParamTy,
}

#[derive(Debug, Encode, Decode)]
pub enum InterfaceParamTy {
    I32 = 0,
}
