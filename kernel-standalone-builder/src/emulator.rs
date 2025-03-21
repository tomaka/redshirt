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

use std::{fs, io, path::Path, process::Command};
use tempdir::TempDir;

/// Configuration for running the kernel in an emulator.
#[derive(Debug)]
pub struct Config<'a> {
    /// Path to the `Cargo.toml` of the standalone kernel.
    pub kernel_cargo_toml: &'a Path,

    /// If true, compiles with `--release`.
    pub release: bool,

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
            fs::create_dir_all(build_dir.path().join("image").join("efi").join("boot"))
                .map_err(Error::Io)?;

            let build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: cfg.kernel_cargo_toml,
                release: cfg.release,
                target_name: "x86_64-unknown-uefi",
                expected_target_suffix: Some("efi"),
                target_specs: None,
                link_script: None,
            })
            .map_err(crate::image::Error::Build)?;

            fs::write(
                build_dir.path().join("OVMF_CODE.fd"),
                &include_bytes!("../res/x86_64-uefi-firmware/OVMF_CODE.fd")[..],
            )
            .map_err(Error::Io)?;
            fs::write(
                build_dir.path().join("OVMF_VARS.fd"),
                &include_bytes!("../res/x86_64-uefi-firmware/OVMF_VARS.fd")[..],
            )
            .map_err(Error::Io)?;
            fs::copy(
                build_out.out_kernel_path,
                build_dir
                    .path()
                    .join("image")
                    .join("efi")
                    .join("boot")
                    .join("bootx64.efi"),
            )
            .map_err(Error::Io)?;

            let status = Command::new("qemu-system-x86_64")
                .args(&["-m", "1024"])
                .args(&["-serial", "stdio"])
                .args(&["-smp", "cpus=4"])
                // TODO: decide whether to enable this ; can cause compatibility issues but runs way faster .args(&["-enable-kvm", "-cpu", "host"])
                .arg("-drive")
                .arg(format!(
                    "if=pflash,format=raw,readonly=on,file={}",
                    build_dir.path().join("OVMF_CODE.fd").display(),
                ))
                .arg("-drive")
                .arg(format!(
                    "if=pflash,format=raw,readonly=on,file={}",
                    build_dir.path().join("OVMF_VARS.fd").display(),
                ))
                .arg("-drive")
                .arg(format!(
                    "format=raw,file=fat:rw:{}",
                    build_dir.path().join("image").display()
                ))
                .status()
                .map_err(Error::EmulatorNotFound)?;

            if !status.success() {
                return Err(Error::EmulatorRunFailure);
            }
        }

        crate::image::Target::RaspberryPi2 => {
            let build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: cfg.kernel_cargo_toml,
                release: cfg.release,
                target_name: "arm-freestanding",
                expected_target_suffix: None,
                target_specs: Some(include_str!("../res/specs/arm-freestanding.json")),
                link_script: Some(include_str!("../res/specs/arm-freestanding.ld")),
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

            if !status.success() {
                return Err(Error::EmulatorRunFailure);
            }
        }

        crate::image::Target::RaspberryPi3 => {
            let build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: cfg.kernel_cargo_toml,
                release: cfg.release,
                target_name: "aarch64-freestanding",
                expected_target_suffix: None,
                target_specs: Some(include_str!("../res/specs/aarch64-freestanding.json")),
                link_script: Some(include_str!("../res/specs/aarch64-freestanding.ld")),
            })
            .map_err(crate::image::Error::Build)?;

            let status = Command::new("qemu-system-aarch64")
                .args(&["-M", "raspi3"])
                .args(&["-m", "1024"])
                .args(&["-serial", "stdio"])
                .arg("-kernel")
                .arg(build_out.out_kernel_path)
                .status()
                .map_err(Error::EmulatorNotFound)?;

            if !status.success() {
                return Err(Error::EmulatorRunFailure);
            }
        }

        crate::image::Target::HiFiveRiscV => {
            let build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: cfg.kernel_cargo_toml,
                release: cfg.release,
                target_name: "riscv-hifive",
                expected_target_suffix: None,
                target_specs: Some(include_str!("../res/specs/riscv-hifive.json")),
                link_script: Some(include_str!("../res/specs/riscv-hifive.ld")),
            })
            .map_err(crate::image::Error::Build)?;

            let status = Command::new("qemu-system-riscv32")
                .args(&["-machine", "sifive_e"])
                .args(&["-cpu", "sifive-e31"])
                .args(&["-m", "2G"])
                .args(&["-serial", "stdio"])
                .arg("-kernel")
                .arg(build_out.out_kernel_path)
                .status()
                .map_err(Error::EmulatorNotFound)?;

            if !status.success() {
                return Err(Error::EmulatorRunFailure);
            }
        }
    }

    Ok(())
}
