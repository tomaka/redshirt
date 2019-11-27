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

use std::process::Command;

fn main() {
    let status = Command::new("cargo")
        .arg("rustc")
        .arg("--release")
        .args(&["--target", "wasm32-wasi"])
        .args(&["--package", "ipfs"])
        .args(&["--bin", "ipfs"])
        .args(&["--manifest-path", "../../modules/ipfs/Cargo.toml"])
        .arg("--")
        .args(&["-C", "link-arg=--export-table"])
        .status()
        .unwrap();
    assert!(status.success());

    // TODO: not a great solution
    for entry in walkdir::WalkDir::new("../../modules/") {
        println!("cargo:rerun-if-changed={}", entry.unwrap().path().display());
    }
    for entry in walkdir::WalkDir::new("../../interfaces/") {
        println!("cargo:rerun-if-changed={}", entry.unwrap().path().display());
    }
}
