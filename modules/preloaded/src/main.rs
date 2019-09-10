#![feature(alloc_error_handler, start)]
#![no_std]

extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;

#[link(wasm_import_module = "")]
extern {
    fn get_random() -> i32;
	fn sbrk(size: u32);
    fn abort() -> !;
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { unsafe { abort() } }

/// Wasm allocator
pub struct Allocator {
    heap: AtomicUsize,
}

#[cfg(not(feature = "no_global_allocator"))]
#[global_allocator]
static ALLOCATOR: Allocator = Allocator { heap: AtomicUsize::new(8) };

mod __impl {
	use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::Ordering;

	use super::Allocator;

	unsafe impl GlobalAlloc for Allocator {
		unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            /*while self.heap % layout.align() != 0 {
                self.heap += 1;     // TODO: lol, too tired to figure out how to do that otherwise
            }*/
            // TODO: must use compare_exchange or something
            let ret = self.heap.fetch_add(layout.size(), Ordering::Relaxed);
            ret as *mut u8
		}

		unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
		}
	}
}

#[alloc_error_handler]
pub extern fn oom(_: core::alloc::Layout) -> ! {
    unsafe { abort() }
	/*static OOM_MSG: &str = "Runtime memory exhausted. Aborting";

	unsafe {
		extern_functions_host_impl::ext_print_utf8(OOM_MSG.as_ptr(), OOM_MSG.len() as u32);
		intrinsics::abort();
	}*/
}

/*#[no_mangle]
pub extern "C" fn load(name: &str) -> Vec<u8> {
    Vec::new()
}*/

/*#[start]
fn main(_: isize, _: *const *const u8) -> isize {
    unsafe { test() };
    0
}*/

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
    //(unsafe { get_random() }) as isize
    //5

    panic!();

    let mut sha3 = tiny_keccak::Keccak::new_sha3_256();
    let data: Vec<u8> = From::from("hello");
    let data2: Vec<u8> = From::from("world");

    sha3.update(&data);
    sha3.update(&[b' ']);
    sha3.update(&data2);

    let mut res: [u8; 32] = [0; 32];
    sha3.finalize(&mut res);

    let expected: &[u8] = &[
        0x64, 0x4b, 0xcc, 0x7e, 0x56, 0x43, 0x73, 0x04, 0x09, 0x99, 0xaa, 0xc8, 0x9e, 0x76, 0x22,
        0xf3, 0xca, 0x71, 0xfb, 0xa1, 0xd9, 0x72, 0xfd, 0x94, 0xa3, 0x1c, 0x3b, 0xfb, 0xf2, 0x4e,
        0x39, 0x38,
    ];

    assert_eq!(&res, expected);
    0
}
