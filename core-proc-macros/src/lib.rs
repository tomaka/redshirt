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

#![feature(proc_macro_span)] // TODO: https://github.com/rust-lang/rust/issues/54725

extern crate proc_macro;

use std::{fs, process::Command};

/// Compiles a WASM module and includes it similar to `include_bytes!`.
/// Must be passed the path to a directory containing a `Cargo.toml`.
#[proc_macro_hack::proc_macro_hack]
pub fn build_wasm_module(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // Find the absolute path requested by the user.
    let wasm_crate_path = {
        let macro_param = syn::parse_macro_input!(tokens as syn::LitStr);
        let macro_call_file = {
            // We go through the stack of Spans until we find one with a file path.
            let mut span = proc_macro::Span::call_site();
            loop {
                let src_file = span.source_file();
                if src_file.is_real() {
                    break src_file.path().parent().unwrap().to_owned();
                }
                span = span.parent().unwrap();
            }
        };

        macro_call_file.join(macro_param.value())
    };

    // Get the package ID of the package requested by the user.
    let pkg_id = {
        let output = Command::new("cargo")
            .arg("read-manifest")
            .current_dir(&wasm_crate_path)
            .output()
            .unwrap();
        assert!(output.status.success());
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        json.as_object()
            .unwrap()
            .get("id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    };

    // Determine the path to the `.wasm` and `.d` files that Cargo will generate.
    let (wasm_output, dependencies_output) = {
        let metadata = cargo_metadata::MetadataCommand::new()
            .current_dir(&wasm_crate_path)
            .no_deps()
            .exec()
            .unwrap();
        let package = metadata
            .packages
            .iter()
            .find(|p| p.id.repr == pkg_id)
            .unwrap();
        let mut bin_targets_iter = package
            .targets
            .iter()
            .filter(|t| t.kind.iter().any(|k| k == "bin"));
        let bin_target = bin_targets_iter.next().unwrap();
        assert!(bin_targets_iter.next().is_none());
        let base = metadata
            .target_directory
            .join("wasm32-unknown-unknown")
            .join("release");
        let wasm = base.join(format!("{}.wasm", bin_target.name));
        let deps = base.join(format!("{}.d", bin_target.name));
        (wasm, deps)
    };

    // Actually build the module.
    assert!(Command::new("cargo")
        .arg("rustc")
        .arg("--release")
        .args(&["--target", "wasm32-unknown-unknown"])
        .arg("--")
        .args(&["-C", "link-arg=--export-table"])
        .current_dir(&wasm_crate_path)
        .status()
        .unwrap()
        .success());

    // Read the list of source files that we must depend upon.
    let dependended_files: Vec<String> = {
        // Read the output `.d` file.
        let dependencies_content = fs::read(dependencies_output).unwrap();
        let mut list_iter = dependencies_content.split(|b| *b == b' ');
        let _ = list_iter.next(); // First entry is the output file.
        list_iter.map(|file| {
            String::from_utf8(file.to_owned()).unwrap()
        }).collect()
    };

    // Read the output `.wasm` file.
    let wasm_content = fs::read(wasm_output).unwrap();

    // TODO: handle if the user renames `redshirt_core` to something else?
    // TODO: use `include_bytes!` for the final wasm instead?
    let rust_out = format!(
        "{{
            const MODULE_BYTES: [u8; {}] = [{}];
            /* {} */
            redshirt_core::module::Module::from_bytes(&MODULE_BYTES[..]).unwrap()
        }}",
        wasm_content.len(),
        wasm_content
            .iter()
            .map(|byte| byte.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        dependended_files.iter()
            .map(|v| format!("include_str!(\"{}\");", v))//.escape_default().to_string()))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    //panic!("{}", rust_out);
    rust_out.parse().unwrap()
}
