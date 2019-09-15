// Copyright(c) 2019 Pierre Krieger

use core::ffi::c_void;

#[link(wasm_import_module = "")]
extern "C" {
    pub(crate) fn register_interface(interface: *const Interface) -> i32;
}

#[repr(C)]
pub struct Interface {
    pub name: *const c_void,
    pub name_len: usize,
    pub fns: *const c_void,
    pub fns_len: usize,
}

#[repr(C)]
pub struct InterfaceFn {
    pub pointer: i32,
    pub params: *const c_void,
    pub params_len: usize,
}

#[repr(C)]
pub struct InterfaceParam {
    pub ty: InterfaceParamTy,
}

#[repr(u32)]
pub enum InterfaceParamTy {
    I32 = 0,
}
