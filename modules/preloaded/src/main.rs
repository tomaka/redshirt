#![feature(start)]
#![no_std]

#[link(wasm_import_module = "")]
extern {
    fn test() -> i32;
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }

#[no_mangle]
pub static FOO: [&'static [u8]; 4] = [b"a", b"b", b"c", b"d"];

#[no_mangle]
pub fn bar() {
    
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
    (unsafe { test() }) as isize
    //5
}
