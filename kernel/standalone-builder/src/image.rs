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

use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::Command,
};
use tempdir::TempDir;

/// Configuration for building a bootable image.
#[derive(Debug)]
pub struct Config<'a> {
    /// Path to the `Cargo.toml` of the standalone kernel.
    pub kernel_cargo_toml: &'a Path,

    /// Path where to write the output image.
    ///
    /// The path must exist, and any existing file will be overwritten.
    pub output_file: &'a Path,

    /// If true, compiles with `--release`.
    pub release: bool,

    /// Platform to compile for.
    pub target: Target,
    // TODO: device type
}

/// Target platform.
#[derive(Debug)]
pub enum Target {
    RaspberryPi2,
    X8664Multiboot2,
}

/// Error that can happen during the build.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error while building the kernel.
    #[error("Error while building the kernel: {0}")]
    Build(#[from] crate::build::Error),

    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Builds a bootable image from a compiled kernel.
pub fn build_image(config: Config) -> Result<(), Error> {
    match config.target {
        Target::X8664Multiboot2 => {
            let build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: config.kernel_cargo_toml,
                release: config.release,
                target_name: "x86_64-multiboot2",
                target_specs: include_str!("../res/specs/x86_64-multiboot2.json"),
                link_script: include_str!("../res/specs/x86_64-multiboot2.ld"),
            })?;

            build_x86_multiboot2_cdrom_iso(build_out.out_kernel_path, config.output_file)?;
            Ok(())
        }

        Target::RaspberryPi2 => {
            let build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: config.kernel_cargo_toml,
                release: config.release,
                target_name: "arm-freestanding",
                target_specs: include_str!("../res/specs/arm-freestanding.json"),
                link_script: include_str!("../res/specs/arm-freestanding.ld"),
            })?;

            unimplemented!()
        }
    }
}

/// Builds an x86 bootable CDROM ISO with a multiboot2 bootloader on it.
///
/// Assumes that the kernel file is an ELF file that can accept multiboot2 information.
fn build_x86_multiboot2_cdrom_iso(
    kernel_path: impl AsRef<Path>,
    output_file: impl AsRef<Path>,
) -> Result<(), io::Error> {
    let build_dir = TempDir::new("redshirt-kernel-iso-build").expect("test0");

    fs::create_dir_all(build_dir.path().join("iso").join("boot").join("grub")).expect("test1");
    fs::copy(
        kernel_path,
        build_dir.path().join("iso").join("boot").join("kernel"),
    ).expect("test2");
    fs::write(
        build_dir
            .path()
            .join("iso")
            .join("boot")
            .join("grub")
            .join("grub.cfg"),
        &br#"
set timeout=5
set default=0

menuentry "redshirt" {
    multiboot2 /boot/kernel
}
            "#[..],
    ).expect("test3");

    let output = Command::new("grub2-mkrescue")
        .arg("-o")
        .arg(output_file.as_ref())
        .arg(build_dir.path().join("iso"))
        .output().expect("test4");

    if !output.status.success() {
        // Note: if `grub2-mkrescue` successfully starts (which is checked above), we assume that
        // any further error is due to a bug in the parameters that we passed to it. It is
        // therefore fine to panic.
        let _ = io::stdout().write_all(&output.stdout);
        let _ = io::stderr().write_all(&output.stderr);
        panic!("Error while executing `grub2-mkrescue`");
    }

    build_dir.close().expect("test5");
    Ok(())
}
