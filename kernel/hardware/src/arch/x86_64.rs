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

use core::convert::TryFrom as _;
use x86_64::structures::port::{PortRead as _, PortWrite as _};

pub unsafe fn write_port_u8(port: u32, data: u8) {
    if let Ok(port) = u16::try_from(port) {
        u8::write_to_port(port, data);
    }
}

pub unsafe fn write_port_u16(port: u32, data: u16) {
    if let Ok(port) = u16::try_from(port) {
        u16::write_to_port(port, data);
    }
}

pub unsafe fn write_port_u32(port: u32, data: u32) {
    if let Ok(port) = u16::try_from(port) {
        u32::write_to_port(port, data);
    }
}

pub unsafe fn read_port_u8(port: u32) -> u8 {
    if let Ok(port) = u16::try_from(port) {
        u8::read_from_port(port)
    } else {
        0
    }
}

pub unsafe fn read_port_u16(port: u32) -> u16 {
    if let Ok(port) = u16::try_from(port) {
        u16::read_from_port(port)
    } else {
        0
    }
}

pub unsafe fn read_port_u32(port: u32) -> u32 {
    if let Ok(port) = u16::try_from(port) {
        u32::read_from_port(port)
    } else {
        0
    }
}
