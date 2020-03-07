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
    io::{self, Read, Seek, SeekFrom, Write},
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
    RaspberryPi3,
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

        Target::RaspberryPi2 | Target::RaspberryPi3 => {
            let v7_build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: config.kernel_cargo_toml,
                release: config.release,
                target_name: "arm-freestanding",
                target_specs: include_str!("../res/specs/arm-freestanding.json"),
                link_script: include_str!("../res/specs/arm-freestanding.ld"),
            })?;

            let v8_build_out = crate::build::build(crate::build::Config {
                kernel_cargo_toml: config.kernel_cargo_toml,
                release: config.release,
                target_name: "aarch64-freestanding",
                target_specs: include_str!("../res/specs/aarch64-freestanding.json"),
                link_script: include_str!("../res/specs/aarch64-freestanding.ld"),
            })?;

            let build_dir = TempDir::new("redshirt-sd-card-build")?;
            crate::binary::elf_to_binary(
                crate::binary::Architecture::Arm,
                v7_build_out.out_kernel_path,
                build_dir.path().join("kernel7.img"),
            )?;
            crate::binary::elf_to_binary(
                crate::binary::Architecture::Aarch64,
                v8_build_out.out_kernel_path,
                build_dir.path().join("kernel8.img"),
            )?;

            let img_file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .truncate(true)
                .create(true)
                .open(config.output_file)
                .unwrap();
            img_file.set_len(1 * 1024 * 1024 * 1024)?;
            let img_file = fscommon::BufStream::new(img_file);
            build_raspberry_pi_sd_card(
                img_file,
                fs::File::open(build_dir.path().join("kernel7.img")).unwrap(),
                fs::File::open(build_dir.path().join("kernel8.img")).unwrap(),
            )?;
            Ok(())
        }
    }
}

/// Builds an x86 bootable CDROM ISO with a multiboot2 bootloader on it.
///
/// Assumes that the kernel file is an ELF file that can accept multiboot2 information.
// TODO: some pure Rust implementation of this one day?
fn build_x86_multiboot2_cdrom_iso(
    kernel_path: impl AsRef<Path>,
    output_file: impl AsRef<Path>,
) -> Result<(), io::Error> {
    let build_dir = TempDir::new("redshirt-kernel-iso-build")?;

    fs::create_dir_all(build_dir.path().join("iso").join("boot").join("grub"))?;
    fs::copy(
        kernel_path,
        build_dir.path().join("iso").join("boot").join("kernel"),
    )?;
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
    )?;

    let output = Command::new("grub-mkrescue")
        .arg("-o")
        .arg(output_file.as_ref())
        .arg(build_dir.path().join("iso"))
        .output();

    let output = if let Ok(output) = output {
        Ok(output)
    } else {
        Command::new("grub2-mkrescue")
            .arg("-o")
            .arg(output_file.as_ref())
            .arg(build_dir.path().join("iso"))
            .output()
    }?;

    if !output.status.success() {
        // Note: if `grub2-mkrescue` successfully starts (which is checked above), we assume that
        // any further error is due to a bug in the parameters that we passed to it. It is
        // therefore fine to panic.
        let _ = io::stdout().write_all(&output.stdout);
        let _ = io::stderr().write_all(&output.stderr);
        panic!("Error while executing `grub2-mkrescue`");
    }

    build_dir.close()?;
    Ok(())
}

/// Writes the content of a bootable SD card to `out`.
///
/// `out` must have pre-allocated space. This function does not grow `out`.
///
/// `kernel_32bits` and `kernel_64bits` are the binary content of respectively the 32bits and
/// 64bits kernels.
// Reference: https://github.com/raspberrypi/noobs/wiki/Standalone-partitioning-explained
fn build_raspberry_pi_sd_card(
    mut out: impl Read + Write + Seek,
    mut kernel_32bits: impl Read,
    mut kernel_64bits: impl Read,
) -> Result<(), io::Error> {
    out.seek(SeekFrom::Start(0))?;

    // We start by writing a MBR to the disk.
    // The MBR (Master Boot Record) is the first section of the SD card, and contains information
    // about the disk, including the paritions table.
    // We create one partition covering the entire disk.
    let mut mbr = mbrman::MBR::new_from(&mut out, 512, [0xff, 0x00, 0x34, 0x56]).unwrap();
    mbr[1] = mbrman::MBRPartitionEntry {
        boot: false,
        first_chs: mbrman::CHS::empty(),
        sys: 0x0c, // FAT32
        last_chs: mbrman::CHS::empty(),
        starting_lba: 1,
        sectors: mbr.disk_size - 1,
    };
    mbr.write_into(&mut out).unwrap();

    // Now wrapping `out` so that it only represents the first partition.
    let mut out = fscommon::StreamSlice::new(out, 512, 512 * u64::from(mbr.disk_size - 1)).unwrap();
    out.seek(SeekFrom::Start(0))?;

    // Format partition as FAT32.
    let format_opts = fatfs::FormatVolumeOptions::new()
        .fat_type(fatfs::FatType::Fat32)
        .volume_id(0x48481111)
        .volume_label(*b"boot       ");
    fatfs::format_volume(&mut out, format_opts)?;

    // Open the file system in order to write out files.
    let fs = fatfs::FileSystem::new(out, fatfs::FsOptions::new())?;

    // Copy the content of `firmware/boot`, plus the kernels, to the FAT32 file system.
    {
        let root_dir = fs.root_dir();

        let local_path = Path::new("res").join("rpi-firmware").join("boot");
        for entry in walkdir::WalkDir::new(&local_path) {
            let entry = entry.unwrap();
            let path = entry.path().strip_prefix(&local_path).unwrap();
            let path_string = path.display().to_string();

            if path_string.is_empty() || &path_string == "." || &path_string == ".." {
                continue;
            }

            if entry.file_type().is_dir() {
                root_dir.create_dir(&path_string).unwrap();
                continue;
            }

            let mut file = root_dir.create_file(&path_string)?;
            io::copy(&mut fs::File::open(local_path.join(path))?, &mut file)?;
        }

        let kernel_32bits = {
            let mut buf = Vec::new();
            kernel_32bits.read_to_end(&mut buf)?;
            buf
        };

        {
            let mut file = root_dir.create_file("kernel.img")?;
            io::copy(&mut io::Cursor::new(&kernel_32bits), &mut file)?;
        }
        {
            let mut file = root_dir.create_file("kernel7.img")?;
            io::copy(&mut io::Cursor::new(&kernel_32bits), &mut file)?;
        }
        {
            let mut file = root_dir.create_file("kernel7l.img")?;
            io::copy(&mut io::Cursor::new(&kernel_32bits), &mut file)?;
        }
        {
            let mut file = root_dir.create_file("kernel8.img")?;
            io::copy(&mut kernel_64bits, &mut file)?;
        }
    }

    fs.unmount()?;
    Ok(())
}
