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

//! Video output interface.
//!
//! This interface serves to register devices capable of presenting an image to the user. Usually
//! a monitor.
//!
//! This interface is extremely naive at the moment. In the future, it should include:
//!
//! - Giving the list of supported video modes, and allowing changing the video mode of the output.
//! - Generic graphics rendering. One would register graphics accelerators, connected to 0 or more
//!   monitors.
//! - Still registering devices in "linear framebuffer mode", for compatibility with VGA/VBE on PC,
//!   or more primitive hardware on embedded devices.
//!
//! The main inspiration for designing "graphics commands" should be Vulkan and WebGPU.
//!

pub mod ffi;
pub mod video_output;
