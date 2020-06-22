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
//! VBE and VGA are two standards specifying a way for interfacing with a video card.
//!
//! The VGA standard was created in the 1980 and defines, amongst other things, a list of
//! I/O registers that the video card must implement.
//!
//! The VBE standard is more recent and defines a list of functions that the video BIOS must
//! provide. It is a superset of VGA.
//!
//! While these standards are fairly old, they are as of 2020 still the most up-to-date
//! non-hardware-specific way of interfacing with a video card, and is still almost universally
//! supported nowadays.
//!
//! More modern features, such as 3D acceleration, are not standardized. They are much more
//! complex to implement and, in practice, require writing a driver specific to each vendor.
//!
//! Both VGa and VBE refer to "the" video card of the machine. In other words, the motherboard
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
//! standardized list of calls. Details can be found [here](https://en.wikipedia.org/wiki/INT_10H).
//!
//! The VBE standards, whose most recent version is 3.0, extends this list of functions. If the
//! video BIOS doesn't support the VBE standard, it is assumed to simply do nothing and return
//! an error code. While the VBE functions are an extension to the VGA functions, they are really
//! meant to entirely replace all the legacy VGA functions.
//!
//! VBE-compatible cards, in addition to interruption 0x10, must also provide a protected-mode
//! (32bits) entry point, which makes it possible to call VBE functions from protected mode.
//! Unfortunately, the requirements for the protected mode entry point involve setting up memory
//! segments that point to specific physcial memory locations. Memory segments are no longer a
//! thing in long mode (64bits).
//!
//! Whatever entry point we decide to this, accessing these functions involves switching the
//! processor mode from long mode (64bits) to either 16bits or 32bits, which is a hassle.
//! For this reason, our solution consists in executing the VBE functions through a real mode
//! (16bits) *emulator*. In other words, we read the instructions contained in the video BIOS
//! and interpret them.
//!
//! > **Note**: We restrict ourselves to a 16bits emulator as it is considerably more simple to
//! >           write than a 32bits emulator, but there is no fundamental reason to prefer 16bits
//! >           over 32bits.
//!

mod interpreter;
mod vbe;

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    // TODO: somehow lock the "VBE system" so that no two drivers try to access it
    // TODO: also this module should be able to run on any platform, and only enable itself on x86

    let mut vbe = vbe::load_vbe_info().await.unwrap();
    // TODO: mode selection
    let mode = vbe
        .modes()
        .find(|m| m.pixels_dimensions().0 > 1500)
        .unwrap()
        .num();
    vbe.set_current_mode(mode).await;
}
