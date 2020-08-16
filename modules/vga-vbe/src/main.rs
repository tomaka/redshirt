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

//! Support for VBE 3.0 and VGA.
//!
//! VBE and VGA are two standards specifying a way of interfacing with a video card.
//!
//! The VGA standard was created in the 1980 and defines, amongst other things, a list of
//! I/O registers that the video card must implement.
//! The VBE standard is more recent and defines a list of functions that the video BIOS must
//! provide. It is a superset of VGA.
//!
//! While these standards are fairly old, they are as of 2020 still the most up-to-date
//! non-hardware-specific way of interfacing with a video card, and are still almost universally
//! supported nowadays.
//!
//! More modern features, such as 3D acceleration, are not standardized. They are much more
//! complex to implement and, in practice, require writing a driver specific to each vendor.
//!
//! Both VGA and VBE refer to "the" video card of the machine. In other words, the motherboard
//! firmware must either choose a main video card or expose all the video cards together as if it
//! was a single one, and the VGA and VBE functions apply on it. It is not possible to support
//! multiple distinct video cards without writing vendor-specific drivers.
//!
//! # VESA Bios Extension (VBE)
//!
//! When the machine is powered up, if the BIOS/firmware detects a VGA-compatible video card, it
//! sets up the entry 0x10 of the IVT (interrupt vector table) to point to the entry point of the
//! video BIOS, mapped somewhere in memory.
//!
//! Amongst other things, being "VGA-compatible" means that the video BIOS must respond to a
//! certain standardized list of calls. More details can be found
//! [here](https://en.wikipedia.org/wiki/INT_10H).
//!
//! The VBE standard, whose most recent version is 3.0, extends this list of functions. If the
//! video BIOS doesn't support the VBE standard, it is assumed to simply do nothing and return
//! an error code. While the VBE functions are an extension to the VGA functions, they are really
//! meant to entirely replace the VGA functions.
//!
//! VBE-compatible cards, in addition to interruption 0x10, must also provide a protected-mode
//! (32bits) entry point to their BIOS, which makes it possible to call VBE functions from
//! protected mode. Unfortunately, the requirements for the protected mode entry point involve
//! setting up memory segments that point to specific physcial memory locations. Memory segments
//! are no longer a thing in long mode (64bits).
//!
//! Whatever entry point we decide to this, accessing these functions would involve switching the
//! processor mode from long mode (64bits) to either 16bits or 32bits, which is a hassle.
//! For this reason, our solution consists in executing the VBE functions through a real mode
//! (16bits) emulator. In other words, we read the instructions contained in the video BIOS
//! and interpret them.
//!
//! > **Note**: We restrict ourselves to a 16bits emulator as it is considerably more simple to
//! >           write than a 32bits emulator, but there is no fundamental reason to prefer 16bits
//! >           over 32bits.
//!

use core::convert::TryFrom as _;

mod interpreter;
mod vbe;

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    let mut pci_locks = Vec::new();

    let pci_devices = redshirt_pci_interface::get_pci_devices().await;
    for device in pci_devices {
        // We match any PCI device that self-describes as VGA-compatible.
        match (device.class_code, device.subclass, device.prog_if) {
            (0x03, 0x00, 0x00) => {}
            _ => continue,
        };

        let pci_lock = match redshirt_pci_interface::PciDeviceLock::lock(device.location).await {
            Ok(l) => l,
            // PCI device is already handled by a different driver.
            Err(_) => continue,
        };

        pci_locks.push(pci_lock);
    }

    if pci_locks.is_empty() {
        return;
    }

    let mut vbe = unsafe { vbe::load_vbe_info().await.unwrap() };

    let (
        chosen_mode_num,
        width,
        height,
        linear_framebuffer_location,
        bytes_per_scan_line,
        red_mask_size,
        red_mask_pos,
        green_mask_size,
        green_mask_pos,
        blue_mask_size,
        blue_mask_pos,
        reserved_mask_size,
        reserved_mask_pos,
    ) = {
        let mut out = None;
        for mode in vbe.modes() {
            if mode.pixels_dimensions().0 < 1500 {
                continue;
            }

            out = Some((
                mode.num(),
                mode.pixels_dimensions().0,
                mode.pixels_dimensions().1,
                mode.linear_framebuffer_location(),
                mode.bytes_per_scan_line(),
                mode.red_mask_size(),
                mode.red_mask_pos(),
                mode.green_mask_size(),
                mode.green_mask_pos(),
                mode.blue_mask_size(),
                mode.blue_mask_pos(),
                mode.reserved_mask_size(),
                mode.reserved_mask_pos(),
            ));
        }
        out.unwrap() // TODO: don't unwrap
    };

    assert_eq!(
        (red_mask_size + green_mask_size + blue_mask_size + reserved_mask_size) % 8,
        0
    );
    let bytes_per_character =
        (red_mask_size + green_mask_size + blue_mask_size + reserved_mask_size) / 8;

    vbe.set_current_mode(chosen_mode_num).await.unwrap();

    // Register the framebuffer as a video output.
    let video_output_registration = redshirt_video_output_interface::video_output::register(
        redshirt_video_output_interface::video_output::VideoOutputConfig {
            width: u32::from(width),
            height: u32::from(height),
            // TODO: proper format
            format: redshirt_video_output_interface::ffi::Format::R8G8B8X8,
        },
    )
    .await;

    // TODO: not implemented in the kernel
    // TODO: should *add* a logging method, rather than set it
    /*redshirt_kernel_log_interface::configure_kernel(
        redshirt_kernel_log_interface::KernelLogMethod {
            enabled: false,
            framebuffer: Some(redshirt_kernel_log_interface::ffi::FramebufferInfo {
                address: linear_framebuffer_location,
                width: width.into(),
                height: height.into(),
                pitch: bytes_per_scan_line.into(),
                bytes_per_character,
                format: redshirt_kernel_log_interface::ffi::FramebufferFormat::Rgb {
                    red_size: red_mask_size,
                    red_position: red_mask_pos,
                    green_size: green_mask_size,
                    green_position: green_mask_pos,
                    blue_size: blue_mask_size,
                    blue_position: blue_mask_pos,
                },
            }),
            uart: None,
        },
    )
    .await;*/

    loop {
        let frame = video_output_registration.next_frame().await;
        if frame.changes.is_empty() {
            continue;
        }

        let mut ops = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();

        for change in frame.changes {
            for (y, pixels_row) in change.pixels.into_iter().enumerate() {
                let addr = linear_framebuffer_location
                    + u64::from(
                        (change.screen_y_start + u32::try_from(y).unwrap())
                            * u32::from(bytes_per_scan_line)
                            + change.screen_x_start * u32::from(bytes_per_character),
                    );
                // TODO: check length of pixels_row
                unsafe { ops.write(addr, pixels_row) };
            }
        }

        ops.send();
    }
}
