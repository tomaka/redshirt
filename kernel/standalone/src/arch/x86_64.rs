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

#![cfg(target_arch = "x86_64")]

#[link(name = "boot")]
extern "C" {}

/// Called by `boot.S` after basic set up has been performed.
///
/// When this function is called, a stack has been set up and as much memory space as possible has
/// been identity-mapped (i.e. the virtual memory is equal to the physical memory).
///
/// Since the kernel was loaded by a multiboot2 bootloader, the first parameter is the memory
/// address of the multiboot header.
#[no_mangle]
extern "C" fn after_boot(multiboot_header: usize) -> ! {
    unsafe {
        let _info = multiboot2::load(multiboot_header);
        // TODO: do something with that?

        crate::main()
    }
}
