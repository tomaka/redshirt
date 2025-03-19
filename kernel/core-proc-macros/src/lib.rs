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

use std::{env, fs, path::Path, process::Command};

/// Turns a string of WebAssembly text representation into a binary representation.
#[proc_macro]
pub fn wat_to_bin(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let wat = syn::parse_macro_input!(tokens as syn::LitStr);
    let wat = wat.value();

    let wasm = match wat::parse_bytes(wat.as_ref()) {
        Ok(w) => w,
        Err(err) => {
            return format!(
                "compile_error!(\"Failed to convert WAT to Wasm.\n\n{}\")",
                err
            )
            .parse()
            .unwrap();
        }
    };

    // Final output.
    let rust_out = format!(
        "{{
            const MODULE_BYTES: [u8; {}] = [{}];
            &MODULE_BYTES[..]
        }}",
        wasm.len(),
        wasm.iter()
            .map(|c| format!("0x{:x}", c))
            .collect::<Vec<String>>()
            .join(", "),
    );

    // Uncomment to debug.
    //panic!("{}", rust_out);

    rust_out.parse().unwrap()
}
