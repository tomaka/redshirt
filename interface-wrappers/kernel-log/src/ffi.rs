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

/// Communication between a process and the interface handler.
///
/// A message consists of either:
///
/// - A `0` byte followed with a UTF-8 log message.
/// - A `1` byte followed with a SCALE-codec-encoded [`KernelLogMethod`].
///
use parity_scale_codec::{Decode, Encode};
use redshirt_syscalls::InterfaceHash;

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0xcd, 0xba, 0x59, 0xb1, 0xb6, 0x5e, 0xd4, 0x9a, 0xfe, 0x25, 0xd0, 0x7c, 0x04, 0x1f, 0xae, 0x82,
    0x5b, 0xf3, 0xc9, 0xca, 0x89, 0x48, 0x81, 0xe0, 0x3b, 0x3a, 0xd2, 0x76, 0x29, 0x04, 0x21, 0x1b,
]);

/// How the kernel should log messages.
#[derive(Debug, Clone, Encode, Decode)]
pub struct KernelLogMethod {
    /// If `true`, log messages should be shown. If `false`, they should be buffered up (to a
    /// certain limit) and will be shown as soon as `enabled` is true.
    pub enabled: bool,

    /// If `Some`, the logs will be printed on a video framebuffer.
    pub framebuffer: Option<FramebufferInfo>,

    /// If `Some`, logs will be emitted on an UART.
    pub uart: Option<UartInfo>,
}

/// Information about how the kernel should print on an UART.
///
/// In order to write, the kernel should repeatidly read a value from `wait_address` until
/// its value, when AND-ed with `wait_mask`, is equal to `wait_compare_equal_if_ready`. Then it
/// should write a value to `write_address`.
#[derive(Debug, Clone, Encode, Decode)]
pub struct UartInfo {
    /// Where to read the value to compare.
    pub wait_address: UartAccess,
    /// Mask to apply (by AND'ing) to the value read from `wait_address`.
    pub wait_mask: u32,
    /// Compares the value (after the mask is applied) with this reference value. If equal, the
    /// UART is ready.
    pub wait_compare_equal_if_ready: u32,
    /// Where to write the output when ready.
    pub write_address: UartAccess,
}

/// How to access either the value to compare or where to write the output.
#[derive(Debug, Clone, Encode, Decode)]
pub enum UartAccess {
    /// 32bits at a specific memory location.
    MemoryMappedU32(u64),
    /// Single byte (8bits) on I/O port. Typically on x86/amd64 systems.
    IoPortU8(u16),
}

/// Information about how the kernel should print on the framebuffer.
#[derive(Debug, Clone, Encode, Decode)]
pub struct FramebufferInfo {
    /// Location in physical memory where the framebuffer starts.
    pub address: u64,
    /// Width of the screen, either in pixels or characters.
    pub width: u32,
    /// Height of the screen, either in pixels or characters.
    pub height: u32,
    /// In order to reach the second line of pixels or characters, one has to advance this number of bytes.
    pub pitch: u64,
    /// Number of bytes a pixel or a character occupies in memory.
    pub bytes_per_character: u8,
    /// Format of the framebuffer's data.
    pub format: FramebufferFormat,
}

/// Format of the framebuffer's data.
#[derive(Debug, Clone, Encode, Decode)]
pub enum FramebufferFormat {
    /// One ASCII character followed with one byte of characteristics.
    Text,
    Rgb {
        red_size: u8,
        red_position: u8,
        green_size: u8,
        green_position: u8,
        blue_size: u8,
        blue_position: u8,
    },
}
