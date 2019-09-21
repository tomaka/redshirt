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
}
