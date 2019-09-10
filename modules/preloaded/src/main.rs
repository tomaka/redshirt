#![feature(alloc_error_handler, start)]
#![no_std]

extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;

#[link(wasm_import_module = "")]
extern {
    fn get_random() -> i32;
    fn abort() -> !;
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { unsafe { abort() } }

#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[alloc_error_handler]
pub extern fn oom(_: core::alloc::Layout) -> ! {
    unsafe { abort() }
}

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
    //(unsafe { get_random() }) as isize
    //5

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
