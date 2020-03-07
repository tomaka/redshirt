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

//! Collection of commands that can build a kernel.
//!
//! # Kernel environment
//!
//! This crate doesn't contain the source code of the kernel. Instead, many of the commands require
//! you to pass the location of a `Cargo.toml` that will build this kernel.
//!
//! This crate, however, is responsible for building bootable images and setting up the boot
//! process on various targets. It therefore sets up an environment that the kernel can expect
//! to be there.
//!
//! This environment is the following:
//!
//! - The kernel must provide a symbol named `_start`. Execution will jump to this symbol, after
//! which the kernel is in total control of the hardware.
//! - The kernel cannot make any assumption about the state of the registers, memory, or hardware
//! when `_start` is executed, with some exceptions depending on the target.
//! - The symbols `__bss_start` and `__bss_end` exist and correspond to the beginning and end
//! of the BSS section (see below).
//!
//! ## BSS section
//!
//! The BSS section is the section, in an ELF binary, where all the static variables whose initial
//! value is all zeroes are located.
//!
//! Normally, it is the role of the ELF loader (e.g. the Linux kernel) to ensure that this section
//! is initialized with zeroes. Operating systems, however, are generally not loaded by an ELF
//! loader.
//!
//! Consequently, when the kernel starts, it **must** write the memory between the `__bss_start`
//! and `__bss_end` symbols with all zeroes.
//!
//! This can be done like this:
//!
//! ```norun
//! let mut ptr = __bss_start;
//! while ptr < __bss_end {
//!     ptr.write_volatile(0);
//!     ptr = ptr.add(1);
//! }
//!
//! extern "C" {
//!     static mut __bss_start: *mut u8;
//!     static mut __bss_end: *mut u8;
//! }
//! ```

pub mod binary;
pub mod build;
pub mod image;

/*fn run_arm(kernel_path: &Path) {
    let build_dir = TempDir::new("redshirt-kernel-arm").unwrap();
    fs::write(
        build_dir.path().join("device.dtb"),
        &include_bytes!("res/bcm2710-rpi-2-b.dtb")[..],
    )
    .unwrap();

    let status = Command::new("qemu-system-arm")
        .args(&["-M", "raspi2"])
        .args(&["-m", "1024"])
        .args(&["-serial", "stdio"])
        .arg("-kernel")
        .arg(kernel_path)
        .arg("-dtb")
        .arg(build_dir.path().join("device.dtb"))
        .status()
        .unwrap();
    assert!(status.success());

    build_dir.close().unwrap();
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
        &include_bytes!("res/grub.cfg")[..],
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
}*/
