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
//! memory-mapped registers that the video card must implement.
//!
//! The VBE standard is more recent and defines a list of functions that the video BIOS must
//! provide. It is a superset of VGA.
//!
//! While these standards are fairly old, they are most up-to-date non-hardware-specific way of
//! interfacing with a video card, and is still almost universally supported nowadays (as of
//! 2020).
//!
//! More modern features, such as 3D acceleration, are not standardized. They are much more
//! complex to implement and often require writing a driver specific to each vendor.
//!
//! Also note that all these standards refer to "the" video card of the machine. In other words,
//! the motherboard firmware must choose a main video card, and the VGA and VBE functions apply
//! on it. It is not possible to support multiple video cards without writing vendor-specific
//! hardware.
//!
//! # VESA Bios Extension (VBE)
//!
//! The most recent way of interfacing with a video card is the VBE 3.0 standard.
//! This standard defines a list of functions that the BIOS must define.
//!
//! VBE functions must support both a real mode (16bits) entry point through interrupt 0x10, as
//! well as a protected mode (32bits) entry point. Unfortunately, the requirements for the
//! protected mode entry point involve setting up memory segments that point to specific physcial
//! memory locations. Memory segments are no longer a thing in long mode (64bits). In other
//! words, in order to access these functions, we would have to switch the processor mode to
//! something else.
//!
//! For this reason, the most sane way to call these functions is to execute them through a real
//! mode (16bits) emulator. In other words, we read the instructions contained in the video BIOS
//! and interpret them.
//!
//! > **Note**: We restrict ourselves to a 16bits emulator because it is considerably more simple
//! >           to write than a 32bits emulator.
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
