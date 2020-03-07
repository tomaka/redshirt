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
    str::FromStr,
};
use structopt::StructOpt;
use tempdir::TempDir;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "redshirt-standalone-builder",
    about = "Redshirt standalone kernel builder and tester."
)]
enum CliOptions {
    /// Runs a pre-compiled kernel with QEMU.
    Qemu {
        /// Kernel file to run.
        #[structopt(parse(from_os_str))]
        kernel_file: PathBuf,

        /// Target triplet the kernel was compiled with.
        #[structopt(long)]
        target: String,
    },

    /// Builds a bootable image.
    ///
    /// Test doc more
    BuildImage {
        /// Location of the Cargo.toml of the standalone kernel.
        ///
        /// If no value is passed, this the file structure is the one of the upstream repository
        /// and try to find the path in a sibling directory.
        #[structopt(long, parse(from_os_str))]
        kernel_cargo_toml: Option<PathBuf>,

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
        ///
        /// Can be one of: `arm-rpi2`, `x86_64-multiboot2`.
        #[structopt(long)]
        target: Target,
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
    RaspberryPi2,
    X8664Multiboot2,
}

impl FromStr for Target {
    type Err = String; // TODO:

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "arm-rpi2" => Ok(Target::RaspberryPi2),
            "x86_64-multiboot2" => Ok(Target::X8664Multiboot2),
            _ => Err("unrecognized target".to_string()),
        }
    }
}

fn main() {
    let cli_opts = CliOptions::from_args();

    match cli_opts {
        CliOptions::BuildImage { kernel_cargo_toml, out, device_type, target } => {
            redshirt_standalone_builder::image::build_image(redshirt_standalone_builder::image::Config {
                kernel_cargo_toml: &kernel_cargo_toml.unwrap(),     // TODO: autodetect
                output_file: &out,
            }).unwrap();
        },
        CliOptions::Qemu { .. } => unimplemented!(),
    }
}
