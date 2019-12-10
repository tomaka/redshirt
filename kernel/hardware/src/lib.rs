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

//! Implements the `hardware` interface.
//!
//! The `hardware` interface is particular in that it can only be implemented using a "hosted"
//! implementation.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::{convert::TryFrom as _, marker::PhantomData};
use nametbd_hardware_interface::ffi::{HardwareMessage, HardwareAccessResponse, Operation};
use parity_scale_codec::{DecodeAll, Encode as _};
use x86_64::structures::port::{PortWrite as _, PortRead as _};

/// State machine for `hardware` interface messages handling.
pub struct HardwareHandler<TMsgId> {
    marker: PhantomData<TMsgId>,
}

impl<TMsgId> HardwareHandler<TMsgId>
where
    TMsgId: Send + 'static,
{
    /// Initializes the new state machine for hardware accesses.
    pub fn new() -> Self {
        HardwareHandler {
            marker: PhantomData,
        }
    }

    /// Processes a message on the `hardware` interface, and optionally returns an answer to
    /// immediately send  back.
    pub fn hardware_message(&self, message_id: Option<TMsgId>, message: &[u8]) -> Option<Vec<u8>> {
        match HardwareMessage::decode_all(&message).unwrap() {
            // TODO: don't unwrap
            HardwareMessage::HardwareAccess(operations) => {
                let mut response = Vec::with_capacity(operations.len());
                for operation in operations {
                    unsafe {
                        if let Some(outcome) = perform_operation(operation) {
                            response.push(outcome);
                        }
                    }
                }

                if !response.is_empty() {
                    Some(response.encode())
                } else {
                    None
                }
            },
            HardwareMessage::InterruptWait(int_id) => unimplemented!(),     // TODO:
        }
    }

    /*/// Returns the next message to answer, and the message to send back.
    pub fn next_answer(&self) -> impl Future<Output = (TMsgId, Vec<u8>)> {
        
    }*/
}

unsafe fn perform_operation(operation: Operation) -> Option<HardwareAccessResponse> {
    match operation {
        Operation::PhysicalMemoryWrite { address, data } => {
            if let Ok(address) = usize::try_from(address) {
                for (off, byte) in data.iter().enumerate() {
                    // TODO: `offset` might be unsound
                    (address as *mut u8).offset(off as isize).write_volatile(*byte);
                }
            }
            None
        },
        Operation::PhysicalMemoryRead { address, len } => {
            let mut out = Vec::with_capacity(len as usize);     // TODO: don't use `as`
            for n in 0..len {
                // TODO: `offset` might be unsound
                out.push((address as *mut u8).offset(n as isize).read_volatile());
            }
            Some(HardwareAccessResponse::PhysicalMemoryRead(out))
        },
        Operation::PortWriteU8 { port, data } => {
            if let Ok(port) = u16::try_from(port) {
                u8::write_to_port(port, data);
            }
            None
        },
        Operation::PortWriteU16 { port, data } => {
            if let Ok(port) = u16::try_from(port) {
                u16::write_to_port(port, data);
            }
            None
        },
        Operation::PortWriteU32 { port, data } => {
            if let Ok(port) = u16::try_from(port) {
                u32::write_to_port(port, data);
            }
            None
        },
        Operation::PortReadU8 { port } => {
            if let Ok(port) = u16::try_from(port) {
                Some(HardwareAccessResponse::PortReadU8(u8::read_from_port(port)))
            } else {
                Some(HardwareAccessResponse::PortReadU8(0))
            }
        },
        Operation::PortReadU16 { port } => {
            if let Ok(port) = u16::try_from(port) {
                Some(HardwareAccessResponse::PortReadU16(u16::read_from_port(port)))
            } else {
                Some(HardwareAccessResponse::PortReadU16(0))
            }
        },
        Operation::PortReadU32 { port } => {
            if let Ok(port) = u16::try_from(port) {
                Some(HardwareAccessResponse::PortReadU32(u32::read_from_port(port)))
            } else {
                Some(HardwareAccessResponse::PortReadU32(0))
            }
        },
    }
}
