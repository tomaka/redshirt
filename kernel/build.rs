// Copyright(c) 2019 Pierre Krieger

use std::process::Command;

fn main() {
    let status = Command::new("cargo")
        //.arg("+nightly")
        .arg("rustc")
        .arg("--release")
        .args(&["--target", "wasm32-wasi"])
        .args(&["--package", "ipfs"])
        .args(&["--bin", "ipfs"])
        .args(&["--manifest-path", "../modules/ipfs/Cargo.toml"])
        .arg("--")
        .args(&["-C", "link-arg=--export-table"])
        .status()
        .unwrap();
    assert!(status.success());

    // TODO: doesn't work if a dependency changes
    for entry in walkdir::WalkDir::new("../modules/ipfs") {
        println!("cargo:rerun-if-changed={}", entry.unwrap().path().display());
    }
}
