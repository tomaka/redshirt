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

#![cfg_attr(feature = "nightly", feature(proc_macro_span))] // TODO: https://github.com/rust-lang/rust/issues/54725

use std::{env, fs, path::Path, process::Command};

/// Turns a string of WebAssembly text representation into a binary representation.
#[proc_macro_hack::proc_macro_hack]
pub fn wat_to_bin(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let wat = syn::parse_macro_input!(tokens as syn::LitStr);
    let wat = wat.value();
    let wasm = wat::parse_bytes(wat.as_ref()).unwrap();

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

/// Compiles a WASM module and includes it similar to `include_bytes!`.
///
/// Must be passed the path to a directory containing a `Cargo.toml`.
/// Can be passed an optional second argument containing the binary name to compile. Mandatory if
/// the package contains multiple binaries.
// TODO: show better errors
#[cfg(feature = "nightly")]
#[proc_macro_hack::proc_macro_hack]
pub fn build_wasm_module(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // Find the absolute path requested by the user, and optionally the binary target.
    let (wasm_crate_path, requested_bin_target) = {
        struct Params {
            path: String,
            bin_target: Option<String>,
        }

        impl syn::parse::Parse for Params {
            fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
                let path: syn::LitStr = input.parse()?;
                let bin_target = if input.is_empty() {
                    None
                } else {
                    let _: syn::Token![,] = input.parse()?;
                    let bin_target: syn::LitStr = input.parse()?;
                    Some(bin_target)
                };

                Ok(Params {
                    path: path.value(),
                    bin_target: bin_target.map(|s| s.value()),
                })
            }
        }

        let macro_params = syn::parse_macro_input!(tokens as Params);
        let macro_call_file = {
            // We go through the stack of Spans until we find one with a file path.
            let mut span = proc_macro::Span::call_site();
            // For hacky reasons, we go two stacks up to find the call site.
            span = span.parent().unwrap();
            span = span.parent().unwrap();
            loop {
                let src_file = span.source_file();
                if src_file.is_real() {
                    break src_file.path().parent().unwrap().to_owned();
                }
                span = span.parent().unwrap();
            }
        };

        (
            macro_call_file.join(macro_params.path),
            macro_params.bin_target,
        )
    };

    // Get the package ID of the package requested by the user.
    let pkg_id = {
        let output = Command::new(env::var("CARGO").unwrap())
            .arg("read-manifest")
            .current_dir(&wasm_crate_path)
            .output()
            .expect("Failed to execute `cargo read-manifest`");
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
    let (wasm_output, dependencies_output, bin_target) = {
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
        let bin_target = if let Some(requested_bin_target) = requested_bin_target {
            match bin_targets_iter.find(|t| t.name == requested_bin_target) {
                Some(t) => t.name.clone(),
                None => panic!("Can't find binary target {:?}", requested_bin_target),
            }
        } else {
            let target = bin_targets_iter.next().unwrap();
            if bin_targets_iter.next().is_some() {
                panic!(
                    "Multiple binary targets available, please mention the one you want: {:?}",
                    package
                        .targets
                        .iter()
                        .filter(|t| t.kind.iter().any(|k| k == "bin"))
                        .map(|t| &t.name)
                        .collect::<Vec<_>>()
                );
            }
            target.name.clone()
        };
        let base = metadata
            .target_directory
            .join("wasm32-wasi")
            .join("release");
        let wasm = base.join(format!("{}.wasm", bin_target));
        let deps = base.join(format!("{}.d", bin_target));
        (wasm, deps, bin_target)
    };

    // Actually build the module.
    let build_status = Command::new(env::var("CARGO").unwrap())
        .arg("rustc")
        .args(&["--bin", &bin_target])
        .arg("--release")
        .args(&["--target", "wasm32-wasi"])
        .arg("--")
        .args(&["-C", "link-arg=--export-table"])
        .args(&["-C", "link-arg=--import-memory"])
        .current_dir(&wasm_crate_path)
        .status()
        .unwrap();
    assert!(build_status.success());

    // Read the list of source files that we must depend upon.
    let dependended_files: Vec<String> = {
        // Read the output `.d` file.
        let dependencies_content = fs::read_to_string(dependencies_output).unwrap();
        let mut list_iter = dependencies_content.lines().next().unwrap().split(" ");
        let _ = list_iter.next(); // First entry is the output file.
                                  // TODO: this is missing Cargo.tomls and stuff I think
        list_iter
            .filter_map(|file| {
                if Path::new(file).exists() {
                    // TODO: figure out why some files are missing
                    Some(file.to_owned())
                } else {
                    None
                }
            })
            .collect()
    };

    // Final output.
    // TODO: handle if the user renames `redshirt_core` to something else?
    let rust_out = format!(
        "{{
            const MODULE_BYTES: &'static [u8] = include_bytes!(\"{}\");
            {}
            redshirt_core::module::Module::from_bytes(&MODULE_BYTES[..]).unwrap()
        }}",
        wasm_output
            .display()
            .to_string()
            .escape_default()
            .to_string(),
        dependended_files
            .iter()
            .map(|v| format!("include_str!(\"{}\");", v.escape_default().to_string()))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    // Uncomment to debug.
    //panic!("{}", rust_out);

    rust_out.parse().unwrap()
}
