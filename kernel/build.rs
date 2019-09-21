// Copyright(c) 2019 Pierre Krieger

use std::process::Command;

fn main() {
    let status = Command::new("cargo")
        //.arg("+nightly")
        .arg("build")
        .arg("--release")
        .args(&["--target", "wasm32-unknown-unknown"])
        .args(&["--package", "ipfs"])
        .status()
        .unwrap();
    assert!(status.success());

    // TODO: doesn't work if a dependency changes
    for entry in walkdir::WalkDir::new("../modules/ipfs") {
        println!("cargo:rerun-if-changed={}", entry.unwrap().path().display());
    }
}
