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

use std::{env, process::Command};

fn main() {
    // Builds additional platform-specific code to link to the kernel.
    let target = env::var("TARGET").unwrap();
    if target.starts_with("x86_64-") {
        cc::Build::new()
            .file("src/arch/x86_64/boot.S")
            .include("src")
            .compile("libboot.a");
    } else if target.starts_with("arm") || target.starts_with("aarch64") {
        // Nothing more to do.
    } else {
        panic!("Unsupported target: {:?}", target)
    }
}
