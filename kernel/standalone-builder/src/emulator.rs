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

/// Configuration for running the kernel in an emulator.
#[derive(Debug)]
pub struct Config<'a> {
    /// Path to the `Cargo.toml` of the standalone kernel.
    pub kernel_cargo_toml: &'a Path,

    /// Which emulator to use.
    pub emulator: Emulator,

    /// Target platform.
    pub target: crate::image::Target,
}

/// Which emulator to use.
#[derive(Debug)]
pub enum Emulator {
    Qemu,
}

/// Error that can happen during the build.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error while building the image.
    #[error("Error while building the image: {0}")]
    Build(#[from] crate::image::Error),

    #[error("Emulator not found: {0}")]
    EmulatorNotFound(io::Error),

    #[error("Emulator run failed")]
    EmulatorRunFailure,

    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Runs the kernel in an emulator.
pub fn run_kernel(cfg: Config) -> Result<(), Error> {
    let Emulator::Qemu = cfg.emulator;

    match cfg.target {
        crate::image::Target::X8664Multiboot2 => {
            let build_dir = TempDir::new("redshirt-kernel-temp-loc")?;
            crate::image::build_image(crate::image::Config {
                kernel_cargo_toml: cfg.kernel_cargo_toml,
                output_file: &build_dir.path().join("image"),
                target: cfg.target,
            })?;

            let status = Command::new("qemu-system-x86_64")
                .args(&["-m", "1024"])
                .arg("-cdrom")
                .arg(build_dir.path().join("image"))
                .args(&["-netdev", "bridge,id=nd0,br=virbr0"])
                .args(&["-device", "ne2k_pci,netdev=nd0"])
                .args(&["-smp", "cpus=4"])
                .status()
                .map_err(Error::EmulatorNotFound)?;
            // TODO: stdout/stderr

            if !status.success() {
                return Err(Error::EmulatorRunFailure);
            }
        }

        crate::image::Target::RaspberryPi2 => {
            let build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: cfg.kernel_cargo_toml,
                release: true,
                target_name: "arm-freestanding",
                target_specs: include_str!("../res/specs/arm-freestanding.json"),
                link_script: include_str!("../res/specs/arm-freestanding.ld"),
            })
            .map_err(crate::image::Error::Build)?;

            let status = Command::new("qemu-system-arm")
                .args(&["-M", "raspi2"])
                .args(&["-m", "1024"])
                .args(&["-serial", "stdio"])
                .arg("-kernel")
                .arg(build_out.out_kernel_path)
                .status()
                .map_err(Error::EmulatorNotFound)?;
            // TODO: stdout/stderr

            if !status.success() {
                return Err(Error::EmulatorRunFailure);
            }
        }
    }

    Ok(())
}
