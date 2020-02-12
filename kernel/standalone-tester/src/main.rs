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

//! Runs a kernel.
//!
//! Runs a kernel using an emulator, if possible.

use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::Command,
};
use structopt::StructOpt;
use tempdir::TempDir;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "redshirt-standalone-tester",
    about = "Redshirt standalone kernel tester."
)]
struct CliOptions {
    /// Target triplet the kernel was compiled with.
    #[structopt(long)]
    target: String,

    /// Kernel file to run.
    #[structopt(parse(from_os_str))]
    kernel_file: PathBuf,
}

fn main() {
    let cli_opts = CliOptions::from_args();

    match cli_opts.target.as_str() {
        "arm-freestanding" => {
            run_arm(&cli_opts.kernel_file);
        }
        "x86_64-multiboot2" => {
            run_x86_64(&cli_opts.kernel_file);
        }
        _ => {
            eprintln!("Unrecognized target: {}", cli_opts.target);
            return;
        }
    }
}

fn run_arm(kernel_path: &Path) {
    let status = Command::new("qemu-system-arm")
        .args(&["-M", "raspi2"])
        .args(&["-m", "1024"])
        .args(&["-serial", "stdio"])
        .arg("-kernel")
        .arg(kernel_path)
        .status()
        .unwrap();
    assert!(status.success());
}

fn run_x86_64(kernel_path: &Path) {
    let build_dir = TempDir::new("redshirt-kernel-iso-build").unwrap();

    fs::create_dir_all(build_dir.path().join("iso").join("boot").join("grub")).unwrap();
    fs::copy(
        kernel_path,
        build_dir.path().join("iso").join("boot").join("kernel"),
    )
    .unwrap();
    fs::write(
        build_dir
            .path()
            .join("iso")
            .join("boot")
            .join("grub")
            .join("grub.cfg"),
        &include_bytes!("grub.cfg")[..],
    )
    .unwrap();

    let output = Command::new("grub2-mkrescue")
        .arg("-o")
        .arg(build_dir.path().join("cdrom.iso"))
        .arg(build_dir.path().join("iso"))
        .output()
        .unwrap();
    if !output.status.success() {
        io::stdout().write_all(&output.stdout).unwrap();
        io::stderr().write_all(&output.stderr).unwrap();
        panic!("Error while executing `grub2-mkrescue`");
    }

    let status = Command::new("qemu-system-x86_64")
        .args(&["-m", "1024"])
        .arg("-cdrom")
        .arg(build_dir.path().join("cdrom.iso"))
        .args(&["-netdev", "bridge,id=nd0,br=virbr0"])
        .args(&["-device", "ne2k_pci,netdev=nd0"])
        .args(&["-smp", "cpus=4"])
        .status()
        .unwrap();
    assert!(status.success());

    build_dir.close().unwrap();
}
