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

//!
//!
//! Reference: https://github.com/raspberrypi/noobs/wiki/Standalone-partitioning-explained

use std::fs;
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::path::Path;

/// Writes the content of a bootable SD card to `out`.
///
/// `out` must have pre-allocated space. This function does not grow `out`.
///
/// `kernel_32bits` and `kernel_64bits` are the binary content of respectively the 32bits and
/// 64bits kernels.
pub fn generate(
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

        let local_path = Path::new("firmware").join("boot");
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
