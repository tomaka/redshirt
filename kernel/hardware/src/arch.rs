// Copyright (C) 2019  Pierre Krieger
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

#[cfg(target_arch = "x86_64")]
use self::x86_64 as platform;

#[cfg(target_arch = "x86_64")]
mod x86_64;

// Functions are duplicated here in order to define a precise API that platforms have to implement.

/// Initialization step.
pub unsafe fn init() {
    platform::init();
}

/// Write data on a specific hardware port. Has no effect if the operation is not supported or the
/// port is out of range.
pub unsafe fn write_port_u8(port: u32, data: u8) {
    platform::write_port_u8(port, data)
}

/// Write data on a specific hardware port. Has no effect if the operation is not supported or the
/// port is out of range.
pub unsafe fn write_port_u16(port: u32, data: u16) {
    platform::write_port_u16(port, data)
}

/// Write data on a specific hardware port. Has no effect if the operation is not supported or the
/// port is out of range.
pub unsafe fn write_port_u32(port: u32, data: u32) {
    platform::write_port_u32(port, data)
}

/// Reads data from a specific hardware port. Returns 0 if the operation is not supported or the
/// port is out of range.
pub unsafe fn read_port_u8(port: u32) -> u8 {
    platform::read_port_u8(port)
}

/// Reads data from a specific hardware port. Returns 0 if the operation is not supported or the
/// port is out of range.
pub unsafe fn read_port_u16(port: u32) -> u16 {
    platform::read_port_u16(port)
}

/// Reads data from a specific hardware port. Returns 0 if the operation is not supported or the
/// port is out of range.
pub unsafe fn read_port_u32(port: u32) -> u32 {
    platform::read_port_u32(port)
}
