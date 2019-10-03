use core::{ffi::c_void, mem, ptr};
use parity_scale_codec::{Decode, Encode};
use std::ffi::CStr;

include!(concat!(env!("OUT_DIR"), "/vk.rs"));

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: [u8; 32] = [
    0x30, 0xc1, 0xd8, 0x90, 0x74, 0x2f, 0x9b, 0x1a, 0x11, 0xfc, 0xcb, 0x53, 0x35, 0xc0, 0x6f, 0xe6,
    0x5c, 0x82, 0x13, 0xe3, 0xcc, 0x04, 0x7b, 0xb7, 0xf6, 0x88, 0x74, 0x1e, 0x7a, 0xf2, 0x84, 0x75, 
];

#[allow(non_camel_case_types)]
pub type PFN_vkAllocationFunction = extern "system" fn(*mut c_void, usize, usize, SystemAllocationScope) -> *mut c_void;
#[allow(non_camel_case_types)]
pub type PFN_vkReallocationFunction = extern "system" fn(*mut c_void, *mut c_void, usize, usize, SystemAllocationScope) -> *mut c_void;
#[allow(non_camel_case_types)]
pub type PFN_vkFreeFunction = extern "system" fn(*mut c_void, *mut c_void);
#[allow(non_camel_case_types)]
pub type PFN_vkInternalAllocationNotification = extern "system" fn(*mut c_void, usize, InternalAllocationType, SystemAllocationScope) -> *mut c_void;
#[allow(non_camel_case_types)]
pub type PFN_vkInternalFreeNotification = extern "system" fn(*mut c_void, usize, InternalAllocationType, SystemAllocationScope) -> *mut c_void;
#[allow(non_camel_case_types)]
pub type PFN_vkDebugReportCallbackEXT = extern "system" fn(DebugReportFlagsEXT, DebugReportObjectTypeEXT, u64, usize, i32, *const i8, *const i8, *mut c_void) -> Bool32;
#[allow(non_camel_case_types)]
pub type PFN_vkVoidFunction = extern "system" fn() -> ();
