// Copyright (C) 2019-2020  Pierre Krieger
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
use core::fmt::{self, Write};

#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        // TODO: somehow freeze all CPUs?
        // Freeze forever.
        loop {
            asm!("wfi");
        }
    }
}
