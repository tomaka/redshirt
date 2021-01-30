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

use std::{
    error,
    path::{Path, PathBuf},
    str::FromStr,
};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "redshirt-standalone-builder",
    about = "Redshirt standalone kernel builder and tester."
)]
enum CliOptions {
    /// Builds and runs the kernel in an emulator.
    EmulatorRun {
        /// Location of the Cargo.toml of the standalone kernel library.
        ///
        /// If no value is passed, this the file structure is the one of the upstream repository
        /// and try to find the path in a sibling directory.
        ///
        /// It is intended that in the future this can be substituted with the path to a build
        /// directory, in which case the standalone kernel library gets fetched from crates.io.
        #[structopt(long, parse(from_os_str))]
        kernel_cargo_toml: Option<PathBuf>,

        /// If passed, compiles with `--release`.
        #[structopt(long)]
        release: bool,

        /// Which target to build for.
        #[structopt(long)]
        target: Target,

        /// Which emulator to use.
        #[structopt(long, default_value = "qemu")]
        emulator: Emulator,
    },

    /// Builds a bootable image.
    BuildImage {
        /// Location of the Cargo.toml of the standalone kernel library.
        ///
        /// If no value is passed, this the file structure is the one of the upstream repository
        /// and try to find the path in a sibling directory.
        ///
        /// It is intended that in the future this can be substituted with the path to a build
        /// directory, in which case the standalone kernel library gets fetched from crates.io.
        #[structopt(long, parse(from_os_str))]
        kernel_cargo_toml: Option<PathBuf>,

        /// If passed, compiles with `--release`.
        #[structopt(long)]
        release: bool,

        /// Path to the output file. Any existing file will be overwritten.
        #[structopt(short, long, parse(from_os_str))]
        out: PathBuf,

        /// What kind of image to generate.
        ///
        /// Can be one of: `cdrom`, `sd-card`.
        ///
        /// Valid values depend on the target. For example, you can't build a CD-ROM targetting
        /// the Raspberry Pi.
        #[structopt(long)]
        device_type: DeviceTy,

        /// Which target to build for.
        #[structopt(long)]
        target: Target,
    },

    /// Builds and test the kernel in an emulator.
    EmulatorTest {
        /// Location of the Cargo.toml of the standalone kernel library.
        ///
        /// If no value is passed, this the file structure is the one of the upstream repository
        /// and try to find the path in a sibling directory.
        ///
        /// It is intended that in the future this can be substituted with the path to a build
        /// directory, in which case the standalone kernel library gets fetched from crates.io.
        #[structopt(long, parse(from_os_str))]
        kernel_cargo_toml: Option<PathBuf>,

        /// Which target to build for.
        #[structopt(long)]
        target: Target,

        /// Which emulator to use.
        #[structopt(long, default_value = "qemu")]
        emulator: Emulator,
    },
}

#[derive(Debug)]
enum DeviceTy {
    Cdrom,
    SdCard,
}

impl FromStr for DeviceTy {
    type Err = String; // TODO:

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cdrom" => Ok(DeviceTy::Cdrom),
            "sd-card" => Ok(DeviceTy::SdCard),
            _ => Err("unrecognized device type".to_string()),
        }
    }
}

#[derive(Debug)]
enum Target {
    HiFiveRiscV,
    RaspberryPi2,
    RaspberryPi3,
    X8664Multiboot2,
}

impl From<Target> for redshirt_standalone_builder::image::Target {
    fn from(target: Target) -> redshirt_standalone_builder::image::Target {
        match target {
            Target::HiFiveRiscV => redshirt_standalone_builder::image::Target::HiFiveRiscV,
            Target::RaspberryPi2 => redshirt_standalone_builder::image::Target::RaspberryPi2,
            Target::RaspberryPi3 => redshirt_standalone_builder::image::Target::RaspberryPi3,
            Target::X8664Multiboot2 => redshirt_standalone_builder::image::Target::X8664Multiboot2,
        }
    }
}

impl FromStr for Target {
    type Err = String; // TODO:

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "arm-rpi2" => Ok(Target::RaspberryPi2),
            "arm-rpi3" => Ok(Target::RaspberryPi3),
            "riscv-hifive" => Ok(Target::HiFiveRiscV),
            "x86_64-multiboot2" => Ok(Target::X8664Multiboot2),
            _ => Err("unrecognized target".to_string()),
        }
    }
}

#[derive(Debug)]
enum Emulator {
    Qemu,
}

impl From<Emulator> for redshirt_standalone_builder::emulator::Emulator {
    fn from(emulator: Emulator) -> redshirt_standalone_builder::emulator::Emulator {
        match emulator {
            Emulator::Qemu => redshirt_standalone_builder::emulator::Emulator::Qemu,
        }
    }
}

impl From<Emulator> for redshirt_standalone_builder::test::Emulator {
    fn from(emulator: Emulator) -> redshirt_standalone_builder::test::Emulator {
        match emulator {
            Emulator::Qemu => redshirt_standalone_builder::test::Emulator::Qemu,
        }
    }
}

impl FromStr for Emulator {
    type Err = String; // TODO:

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "qemu" => Ok(Emulator::Qemu),
            _ => Err("unrecognized emulator".to_string()),
        }
    }
}

fn main() -> Result<(), Box<dyn error::Error + Send + Sync + 'static>> {
    let cli_opts = CliOptions::from_args();

    // Default value for `kernel-cargo-toml` if no value is provided.
    let default_kernel_cargo_toml = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("standalone")
        .join("Cargo.toml");

    match cli_opts {
        CliOptions::BuildImage {
            kernel_cargo_toml,
            release,
            out,
            device_type: _, // TODO: ?!
            target,
        } => {
            redshirt_standalone_builder::image::build_image(
                redshirt_standalone_builder::image::Config {
                    kernel_cargo_toml: &kernel_cargo_toml.unwrap_or(default_kernel_cargo_toml),
                    release,
                    output_file: &out,
                    target: target.into(),
                },
            )?;
        }
        CliOptions::EmulatorRun {
            kernel_cargo_toml,
            release,
            emulator,
            target,
        } => {
            redshirt_standalone_builder::emulator::run_kernel(
                redshirt_standalone_builder::emulator::Config {
                    kernel_cargo_toml: &kernel_cargo_toml.unwrap_or(default_kernel_cargo_toml),
                    release,
                    emulator: emulator.into(),
                    target: target.into(),
                },
            )?;
        }
        CliOptions::EmulatorTest {
            kernel_cargo_toml,
            emulator,
            target,
        } => {
            redshirt_standalone_builder::test::test_kernel(
                redshirt_standalone_builder::test::Config {
                    kernel_cargo_toml: &kernel_cargo_toml.unwrap_or(default_kernel_cargo_toml),
                    emulator: emulator.into(),
                    target: target.into(),
                },
            )?;
            println!("Test successful");
        }
    }

    Ok(())
}
