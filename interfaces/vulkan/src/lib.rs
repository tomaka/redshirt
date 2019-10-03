use core::{ffi::c_void, mem, ptr};
use std::ffi::CStr;

include!(concat!(env!("OUT_DIR"), "/vk.rs"));

pub type PFN_vkVoidFunction = extern "system" fn();

#[allow(non_snake_case)]
pub unsafe extern "C" fn vkGetInstanceProcAddr(instance: usize, name: *const u8) -> PFN_vkVoidFunction {
    let name = match CStr::from_ptr(name as *const _).to_str() {
        Ok(n) => n,
        Err(_) => return mem::transmute(ptr::null::<c_void>())
    };

    panic!("{:?}", name);

    match (instance, name) {
        (0, "vkCreateInstance") => {
            unimplemented!()
        }
        _ => mem::transmute(ptr::null::<c_void>())
    }
}
