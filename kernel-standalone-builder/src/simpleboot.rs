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

use std::{
    ffi::{OsStr, OsString},
    iter,
};

extern "C" {
    fn simpleboot_wrapper(
        argc: std::os::raw::c_int,
        argv: *mut *mut std::os::raw::c_char,
    ) -> std::os::raw::c_int;
}

pub fn run_simpleboot<'a>(args: impl IntoIterator<Item = &'a OsStr>) -> Result<(), ()> {
    let args = iter::once(OsString::from("simpleboot"))
        .chain(args.into_iter().map(|s| s.into()))
        .collect::<Vec<_>>();
    let args_ptrs = args
        .iter()
        .map(|a| a.as_encoded_bytes().as_ptr())
        .collect::<Vec<_>>();

    let out = unsafe {
        simpleboot_wrapper(
            args.len() as std::os::raw::c_int,
            args_ptrs.as_ptr().cast_mut().cast(),
        )
    };

    if out == 0 {
        Ok(())
    } else {
        Err(())
    }
}
