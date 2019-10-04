// Copyright(c) 2019 Pierre Krieger

//! Vulkan bindings.
//!
//! # How it works
//!
//! This library contains an implementation of the Vulkan API v1.1. The [`vkGetInstanceProcAddr`]
//! function is the entry point of the Vulkan API, according to [the Vulkan specifications]
//! (https://www.khronos.org/registry/vulkan/specs/1.1-extensions/html/vkspec.html).
//!
//! The way this implementation works is by serializing all the Vulkan function calls and sending
//! them to the appropriate interface handler. If necessary (i.e. the return type is not `()` or
//! there is a parameter to be written to), the function waits for the answer to come back before
//! returning.
//!
//! From the point of view of the user of Vulkan, this is all that you need to know. Any
//! application that successfully runs on top of Vulkan on the desktop should be able to run on
//! top of  these bindings.
//!
//! Various notes:
//!
//! - For obvious reasons, the `VkAllocationCallbacks` can't work. Considering that the allocation
//!   callbacks exist only for optimization purposes, this parameter is simply ignored (as if it
//!   was null).
//!
//! # From the point of view of the interface handler
//!
//! On the side of the interface handler, the serialized Vulkan function calls have to be
//! handled. The most straight-forward way to do that is by directly handling the messages and
//! sending back answers.
//!
//! Another possibility, however, is to use the [`VulkanRedirect`] struct. The [`VulkanRedirect`]
//! can leverage another implementation of Vulkan (through a `vkGetInstanceProcAddr` function) and
//! can handle incoming messages through the [`VulkanRedirect::handle`] method. Considering
//! the potential instability of these bindings, this is the recommended way to do it.
//!
//! # About items visibility
//!
//! If you look at the source code of this module, you might notice that we generate lots of
//! Vulkan FFI definitions. With the expection of [`vkGetInstanceProcAddr`], though, they are all
//! private.
//!
//! This is because the objective of this module is **not** to provide bindings for Vulkan, but
//! only to provide an implementation of the Vulkan API. Please generate your own bindings.
//!

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
type PFN_vkAllocationFunction = extern "system" fn(*mut c_void, usize, usize, VkSystemAllocationScope) -> *mut c_void;
#[allow(non_camel_case_types)]
type PFN_vkReallocationFunction = extern "system" fn(*mut c_void, *mut c_void, usize, usize, VkSystemAllocationScope) -> *mut c_void;
#[allow(non_camel_case_types)]
type PFN_vkFreeFunction = extern "system" fn(*mut c_void, *mut c_void);
#[allow(non_camel_case_types)]
type PFN_vkInternalAllocationNotification = extern "system" fn(*mut c_void, usize, VkInternalAllocationType, VkSystemAllocationScope) -> *mut c_void;
#[allow(non_camel_case_types)]
type PFN_vkInternalFreeNotification = extern "system" fn(*mut c_void, usize, VkInternalAllocationType, VkSystemAllocationScope) -> *mut c_void;
#[allow(non_camel_case_types)]
type PFN_vkDebugReportCallbackEXT = extern "system" fn(VkDebugReportFlagsEXT, VkDebugReportObjectTypeEXT, u64, usize, i32, *const i8, *const i8, *mut c_void) -> u32;
#[allow(non_camel_case_types)]
type PFN_vkDebugUtilsMessengerCallbackEXT = extern "system" fn(VkDebugUtilsMessageSeverityFlagBitsEXT, VkDebugUtilsMessageTypeFlagsEXT, *const VkDebugUtilsMessengerCallbackDataEXT, *mut c_void) -> u32;
#[allow(non_camel_case_types)]
pub type PFN_vkVoidFunction = extern "system" fn() -> ();

/// Leverages an existing Vulkan implementation to handle [`VulkanMessage`]s.
pub struct VulkanRedirect {
    /// How we retrieve instance proc addresses.
    get_instance_proc_addr: unsafe extern "system" fn(usize, *const u8) -> PFN_vkVoidFunction,
}

impl VulkanRedirect {
    pub fn new(get_instance_proc_addr: unsafe extern "system" fn(usize, *const u8) -> PFN_vkVoidFunction) -> VulkanRedirect {
        VulkanRedirect {
            get_instance_proc_addr,
        }
    }

    /// Handles the given [`VulkanMessage`], optionally producing the answer to send back in
    /// response to this call.
    pub fn handle(&mut self, message: &[u8]) -> Option<Vec<u8>> {
        // TODO: implement, lol
        None
    }
}
