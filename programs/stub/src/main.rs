// Copyright (C) 2019-2021  Pierre Krieger
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

//! Draft for a module, with the purpose of inspecting the WASM output.
//!
//! This module doesn't do much by itself and isn't meant to be actually executed.
//! This code exists with the intent of being compiled in release mode so that one can inspect
//! the WASM output.

#![no_std]
#![no_main]

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(not(any(test, doc, doctest)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}

extern crate alloc;
use alloc::vec;
use futures::prelude::*;

#[no_mangle]
fn _start(_: isize, _: *const *const u8) -> isize {
    redshirt_syscalls::block_on(async_main());
    0
}

fn async_main() -> impl Future<Output = ()> {
    let interface = redshirt_syscalls::InterfaceHash::from_raw_hash([
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16,
        0x17, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35,
        0x36, 0x37,
    ]);

    redshirt_syscalls::next_interface_message().then(move |msg| {
        let msg = match msg {
            redshirt_syscalls::DecodedInterfaceOrDestroyed::Interface(m) => m,
            redshirt_syscalls::DecodedInterfaceOrDestroyed::ProcessDestroyed(_) => panic!(),
        };
        assert_eq!(msg.interface, interface);
        assert_eq!(
            msg.actual_data,
            redshirt_syscalls::EncodedMessage(vec![1, 2, 3, 4, 5, 6, 7, 8])
        );
        future::ready(())
    })
}
