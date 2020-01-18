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

//! Access to physical hardware.
//!
//! Use this interface if you're writing a device driver.

#![deny(intra_doc_link_resolution_failure)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::{vec, vec::Vec};
use futures::prelude::*;

pub mod ffi;
pub mod malloc;

/// Builder for write-only hardware operations.
pub struct HardwareWriteOperationsBuilder {
    operations: Vec<ffi::Operation>,
}

impl HardwareWriteOperationsBuilder {
    pub fn new() -> Self {
        HardwareWriteOperationsBuilder {
            operations: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        HardwareWriteOperationsBuilder {
            operations: Vec::with_capacity(capacity),
        }
    }

    pub unsafe fn memset(&mut self, address: u64, len: u64, value: u8) {
        self.operations.push(ffi::Operation::PhysicalMemoryMemset {
            address,
            len,
            value,
        });
    }

    pub unsafe fn write(&mut self, address: u64, data: impl Into<Vec<u8>>) {
        self.operations.push(ffi::Operation::PhysicalMemoryWriteU8 {
            address,
            data: data.into(),
        });
    }

    pub unsafe fn write_one_u32(&mut self, address: u64, data: u32) {
        self.operations
            .push(ffi::Operation::PhysicalMemoryWriteU32 {
                address,
                data: vec![data],
            });
    }

    pub unsafe fn port_write_u8(&mut self, port: u32, data: u8) {
        self.operations
            .push(ffi::Operation::PortWriteU8 { port, data });
    }

    pub unsafe fn port_write_u16(&mut self, port: u32, data: u16) {
        self.operations
            .push(ffi::Operation::PortWriteU16 { port, data });
    }

    pub unsafe fn port_write_u32(&mut self, port: u32, data: u32) {
        self.operations
            .push(ffi::Operation::PortWriteU32 { port, data });
    }

    pub fn send(self) {
        unsafe {
            if self.operations.is_empty() {
                return;
            }

            let msg = ffi::HardwareMessage::HardwareAccess(self.operations);
            redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &msg).unwrap();
        }
    }
}

/// Writes the given data to the given physical memory address location.
pub unsafe fn write(address: u64, data: impl Into<Vec<u8>>) {
    let mut builder = HardwareWriteOperationsBuilder::with_capacity(1);
    builder.write(address, data);
    builder.send();
}

/// Reads a single `u32` from the given memory address.
#[cfg(feature = "std")]
pub async unsafe fn read_one_u32(address: u64) -> u32 {
    let mut ops = HardwareOperationsBuilder::new();
    let mut out = [0];
    ops.read_u32(address, &mut out);
    ops.send().await;
    out[0]
}

pub unsafe fn write_one_u32(address: u64, data: u32) {
    let mut builder = HardwareWriteOperationsBuilder::with_capacity(1);
    builder.write_one_u32(address, data);
    builder.send();
}

pub unsafe fn port_write_u8(port: u32, data: u8) {
    let mut builder = HardwareWriteOperationsBuilder::with_capacity(1);
    builder.port_write_u8(port, data);
    builder.send();
}

/// Reads the given port.
#[cfg(feature = "std")]
pub async unsafe fn port_read_u8(port: u32) -> u8 {
    let mut builder = HardwareOperationsBuilder::with_capacity(1);
    let mut out = 0;
    builder.port_read_u8(port, &mut out);
    builder.send().await;
    out
}

/// Builder for read and write hardware operations.
pub struct HardwareOperationsBuilder<'a> {
    operations: Vec<ffi::Operation>,
    out: Vec<Out<'a>>,
}

enum Out<'a> {
    MemReadU8(&'a mut [u8]),
    MemReadU16(&'a mut [u16]),
    MemReadU32(&'a mut [u32]),
    PortU8(&'a mut u8),
    PortU16(&'a mut u16),
    PortU32(&'a mut u32),
    Discard,
}

impl<'a> HardwareOperationsBuilder<'a> {
    pub fn new() -> Self {
        HardwareOperationsBuilder {
            operations: Vec::new(),
            out: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        HardwareOperationsBuilder {
            operations: Vec::with_capacity(capacity),
            out: Vec::with_capacity(capacity),
        }
    }

    pub unsafe fn read(&mut self, address: u64, out: &'a mut impl AsMut<[u8]>) {
        let out = out.as_mut();
        self.operations.push(ffi::Operation::PhysicalMemoryReadU8 {
            address,
            len: out.len() as u32, // TODO: don't use `as`
        });
        self.out.push(Out::MemReadU8(out));
    }

    pub unsafe fn read_u32(&mut self, address: u64, out: &'a mut impl AsMut<[u32]>) {
        let out = out.as_mut();
        self.operations.push(ffi::Operation::PhysicalMemoryReadU32 {
            address,
            len: out.len() as u32, // TODO: don't use `as`
        });
        self.out.push(Out::MemReadU32(out));
    }

    pub unsafe fn memset(&mut self, address: u64, len: u64, value: u8) {
        self.operations.push(ffi::Operation::PhysicalMemoryMemset {
            address,
            len,
            value,
        });
    }

    pub unsafe fn write(&mut self, address: u64, data: impl Into<Vec<u8>>) {
        self.operations.push(ffi::Operation::PhysicalMemoryWriteU8 {
            address,
            data: data.into(),
        });
    }

    pub unsafe fn write_one_u32(&mut self, address: u64, data: u32) {
        self.operations
            .push(ffi::Operation::PhysicalMemoryWriteU32 {
                address,
                data: vec![data],
            });
    }

    pub unsafe fn port_write_u8(&mut self, port: u32, data: u8) {
        self.operations
            .push(ffi::Operation::PortWriteU8 { port, data });
    }

    pub unsafe fn port_write_u16(&mut self, port: u32, data: u16) {
        self.operations
            .push(ffi::Operation::PortWriteU16 { port, data });
    }

    pub unsafe fn port_write_u32(&mut self, port: u32, data: u32) {
        self.operations
            .push(ffi::Operation::PortWriteU32 { port, data });
    }

    pub unsafe fn port_read_u8(&mut self, port: u32, out: &'a mut u8) {
        self.operations.push(ffi::Operation::PortReadU8 { port });
        self.out.push(Out::PortU8(out));
    }

    pub unsafe fn port_read_u16(&mut self, port: u32, out: &'a mut u16) {
        self.operations.push(ffi::Operation::PortReadU16 { port });
        self.out.push(Out::PortU16(out));
    }

    pub unsafe fn port_read_u32(&mut self, port: u32, out: &'a mut u32) {
        self.operations.push(ffi::Operation::PortReadU32 { port });
        self.out.push(Out::PortU32(out));
    }

    pub unsafe fn port_read_u8_discard(&mut self, port: u32) {
        self.operations.push(ffi::Operation::PortReadU8 { port });
        self.out.push(Out::Discard);
    }

    pub unsafe fn port_read_u16_discard(&mut self, port: u32) {
        self.operations.push(ffi::Operation::PortReadU16 { port });
        self.out.push(Out::Discard);
    }

    pub unsafe fn port_read_u32_discard(&mut self, port: u32) {
        self.operations.push(ffi::Operation::PortReadU32 { port });
        self.out.push(Out::Discard);
    }

    pub fn send(self) -> impl Future<Output = ()> + 'a {
        unsafe {
            let msg = ffi::HardwareMessage::HardwareAccess(self.operations);
            let out = self.out;
            redshirt_syscalls::emit_message_with_response(&ffi::INTERFACE, msg)
                .unwrap()
                .then(move |response: Vec<ffi::HardwareAccessResponse>| {
                    for (response_elem, out) in response.into_iter().zip(out) {
                        match (response_elem, out) {
                            (_, Out::Discard) => {}
                            (ffi::HardwareAccessResponse::PortReadU8(val), Out::PortU8(out)) => {
                                *out = val
                            }
                            (ffi::HardwareAccessResponse::PortReadU16(val), Out::PortU16(out)) => {
                                *out = val
                            }
                            (ffi::HardwareAccessResponse::PortReadU32(val), Out::PortU32(out)) => {
                                *out = val
                            }
                            (
                                ffi::HardwareAccessResponse::PhysicalMemoryReadU8(val),
                                Out::MemReadU8(out),
                            ) => out.copy_from_slice(&val),
                            (
                                ffi::HardwareAccessResponse::PhysicalMemoryReadU16(val),
                                Out::MemReadU16(out),
                            ) => out.copy_from_slice(&val),
                            (
                                ffi::HardwareAccessResponse::PhysicalMemoryReadU32(val),
                                Out::MemReadU32(out),
                            ) => out.copy_from_slice(&val),
                            _ => unreachable!(),
                        }
                    }

                    future::ready(())
                })
        }
    }
}
