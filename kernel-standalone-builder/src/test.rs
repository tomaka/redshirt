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

use futures::{channel::oneshot, executor, prelude::*};
use std::{
    collections::VecDeque,
    io::{self, Write as _},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use tempdir::TempDir;

/// Configuration for testing the kernel in an emulator.
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

    #[error("Timeout while waiting for success")]
    Timeout,

    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Runs the kernel in an emulator.
pub fn test_kernel(cfg: Config) -> Result<(), Error> {
    let Emulator::Qemu = cfg.emulator;

    match cfg.target {
        crate::image::Target::X8664Multiboot2 => {
            let build_dir = TempDir::new("redshirt-kernel-temp-loc")?;
            crate::image::build_image(crate::image::Config {
                kernel_cargo_toml: cfg.kernel_cargo_toml,
                output_file: &build_dir.path().join("image"),
                release: false,
                target: cfg.target,
            })?;

            run_until_line(
                &mut Command::new("qemu-system-x86_64")
                    .args(&["-m", "1024"])
                    .args(&["-display", "none"])
                    .args(&["-serial", "stdio"])
                    .args(&["-monitor", "none"])
                    .arg("-cdrom")
                    .arg(build_dir.path().join("image"))
                    .args(&["-smp", "cpus=4"]),
            )?;
        }

        crate::image::Target::RaspberryPi2 => {
            let build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: cfg.kernel_cargo_toml,
                release: false,
                target_name: "arm-freestanding",
                target_specs: include_str!("../res/specs/arm-freestanding.json"),
                link_script: include_str!("../res/specs/arm-freestanding.ld"),
            })
            .map_err(crate::image::Error::Build)?;

            run_until_line(
                &mut Command::new("qemu-system-arm")
                    .args(&["-M", "raspi2"])
                    .args(&["-m", "1024"])
                    .args(&["-display", "none"])
                    .args(&["-serial", "stdio"])
                    .args(&["-monitor", "none"])
                    .arg("-kernel")
                    .arg(build_out.out_kernel_path),
            )?;
        }

        crate::image::Target::RaspberryPi3 => {
            let build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: cfg.kernel_cargo_toml,
                release: false,
                target_name: "aarch64-freestanding",
                target_specs: include_str!("../res/specs/aarch64-freestanding.json"),
                link_script: include_str!("../res/specs/aarch64-freestanding.ld"),
            })
            .map_err(crate::image::Error::Build)?;

            run_until_line(
                &mut Command::new("qemu-system-aarch64")
                    .args(&["-M", "raspi3"])
                    .args(&["-m", "1024"])
                    .args(&["-display", "none"])
                    .args(&["-serial", "stdio"])
                    .args(&["-monitor", "none"])
                    .arg("-kernel")
                    .arg(build_out.out_kernel_path),
            )?;
        }

        crate::image::Target::HiFiveRiscV => {
            let build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: cfg.kernel_cargo_toml,
                release: false,
                target_name: "riscv-hifive",
                target_specs: include_str!("../res/specs/riscv-hifive.json"),
                link_script: include_str!("../res/specs/riscv-hifive.ld"),
            })
            .map_err(crate::image::Error::Build)?;

            run_until_line(
                &mut Command::new("qemu-system-riscv32")
                    .args(&["-machine", "sifive_e"])
                    .args(&["-cpu", "sifive-e31"])
                    .args(&["-m", "2G"])
                    .args(&["-display", "none"])
                    .args(&["-serial", "stdio"])
                    .args(&["-monitor", "none"])
                    .arg("-kernel")
                    .arg(build_out.out_kernel_path),
            )?;
        }
    }

    Ok(())
}

fn run_until_line(command: &mut Command) -> Result<(), Error> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())  // TODO: debugging
        .spawn()
        .map_err(Error::EmulatorNotFound)?;

    let stdout = child.stdout.take().unwrap();
    let timeout = futures_timer::Delay::new(Duration::from_secs(30));

    let result = executor::block_on(async move {
        futures::select! {
            outcome = signal_when_line_detected(stdout).fuse() => {
                outcome.map_err(|_| Error::EmulatorRunFailure)
            }
            _ = timeout.fuse() => {
                Err(Error::Timeout)
            }
        }
    });

    // Killing the children, otherwise it stays alive.
    let _ = child.kill();
    result
}

fn signal_when_line_detected(read: impl io::Read + Send + 'static) -> oneshot::Receiver<()> {
    let expected = b"[boot] boot successful";
    let (tx, rx) = oneshot::channel();

    thread::spawn(move || {
        let mut bytes = read.bytes();
        let mut window = (0..expected.len()).map(|_| 0u8).collect::<VecDeque<_>>();

        loop {
            window.pop_front();
            window.push_back(match bytes.next() {
                Some(Ok(b)) => {
                    // TODO: add a CLI option to control this?
                    let _ = io::stdout().write_all(&[b]);
                    b
                }
                _ => return,
            });

            let mut window_iter = window.iter();
            let mut expected_iter = expected.iter();
            loop {
                match (window_iter.next(), expected_iter.next()) {
                    (Some(a), Some(b)) if a == b => continue,
                    (None, None) => {
                        let _ = tx.send(());
                        return;
                    }
                    _ => break,
                }
            }
        }
    });

    rx
}
