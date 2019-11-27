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

extern crate compiler_builtins;

#[link(name = "boot")]
extern {}

// TODO: figure out how to remove these
#[no_mangle]
pub extern "C" fn fmod(a: f64, b: f64) -> f64 {
    libm::fmod(a, b)
}
#[no_mangle]
pub extern "C" fn fmodf(a: f32, b: f32) -> f32 {
    libm::fmodf(a, b)
}
#[no_mangle]
pub extern "C" fn fmin(a: f64, b: f64) -> f64 {
    libm::fmin(a, b)
}
#[no_mangle]
pub extern "C" fn fminf(a: f32, b: f32) -> f32 {
    libm::fminf(a, b)
}
#[no_mangle]
pub extern "C" fn fmax(a: f64, b: f64) -> f64 {
    libm::fmax(a, b)
}
#[no_mangle]
pub extern "C" fn fmaxf(a: f32, b: f32) -> f32 {
    libm::fmaxf(a, b)
}
#[no_mangle]
pub extern "C" fn __truncdfsf2(a: f64) -> f32 {
    libm::trunc(a) as f32   // TODO: correct?
}

#[no_mangle]
extern "C" fn loader_main() -> ! {
    crate::main()
}
