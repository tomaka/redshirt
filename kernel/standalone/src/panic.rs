// Copyright (C) 2019  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use alloc::string::String;
use core::fmt::Write;

#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    // Because the diagnostic code below might panic again, we first print a `Panic` message on
    // the top left of the screen.
    let vga_buffer = 0xb8000 as *mut u8;
    for (i, &byte) in b"Panic".iter().enumerate() {
        unsafe {
            *vga_buffer.offset(i as isize * 2) = byte;
            *vga_buffer.offset(i as isize * 2 + 1) = 0xc;
        }
    }

    let mut console = unsafe { nametbd_x86_stdout::Console::init() };

    if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
        let _ = writeln!(console, "panic occurred: {:?}", s);
    } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
        let _ = writeln!(console, "panic occurred: {:?}", s);
    } else if let Some(message) = panic_info.message() {
        let _ = Write::write_fmt(&mut console, *message);
        let _ = writeln!(console, "");
    } else {
        let _ = writeln!(console, "panic occurred");
    }

    if let Some(location) = panic_info.location() {
        let _ = writeln!(
            console,
            "panic occurred in file '{}' at line {}",
            location.file(),
            location.line()
        );
    } else {
        let _ = writeln!(
            console,
            "panic occurred but can't get location information..."
        );
    }

    crate::arch::halt();
}
