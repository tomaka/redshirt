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

//! Implements the `hardware` interface.
//!
//! The `hardware` interface is particular in that it can only be implemented using a "hosted"
//! implementation.

use crate::arch::PlatformSpecific;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{convert::TryFrom as _, pin::Pin, sync::atomic};
use crossbeam_queue::SegQueue;
use futures::prelude::*;
use hashbrown::HashMap;
use nohash_hasher::BuildNoHashHasher;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_hardware_interface::ffi::{
    HardwareAccessResponse, HardwareMessage, Operation, INTERFACE,
};
use spinning_top::Spinlock;

/// State machine for `hardware` interface messages handling.
pub struct HardwareHandler<TPlat> {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<TPlat>>,
    /// For each PID, a list of memory allocations.
    // TODO: optimize
    allocations: Spinlock<HashMap<Pid, Vec<Vec<u8>>, BuildNoHashHasher<u64>>>,
    /// List of messages waiting to be emitted with `next_event`.
    pending_messages: SegQueue<(MessageId, Result<EncodedMessage, ()>)>,
}

impl<TPlat> HardwareHandler<TPlat> {
    /// Initializes the new state machine for hardware accesses.
    pub fn new(platform_specific: Pin<Arc<TPlat>>) -> Self {
        HardwareHandler {
            registered: atomic::AtomicBool::new(false),
            platform_specific,
            allocations: Spinlock::new(HashMap::default()),
            pending_messages: SegQueue::new(),
        }
    }
}

impl<'a, TPlat> NativeProgramRef<'a> for &'a HardwareHandler<TPlat>
where
    TPlat: PlatformSpecific,
{
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        if !self.registered.swap(true, atomic::Ordering::Relaxed) {
            return Box::pin(future::ready(NativeProgramEvent::Emit {
                interface: redshirt_interface_interface::ffi::INTERFACE,
                message_id_write: None,
                message: redshirt_interface_interface::ffi::InterfaceMessage::Register(INTERFACE)
                    .encode(),
            }));
        }

        // TODO: wrong; if a message gets pushed, we don't wake up the task
        if let Ok((message_id, answer)) = self.pending_messages.pop() {
            Box::pin(future::ready(NativeProgramEvent::Answer {
                message_id,
                answer,
            }))
        } else {
            Box::pin(future::pending())
        }
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        debug_assert_eq!(interface, INTERFACE);

        match HardwareMessage::decode(message) {
            Ok(HardwareMessage::HardwareAccess(operations)) => {
                let mut response = Vec::with_capacity(operations.len());
                for operation in operations {
                    unsafe {
                        if let Some(outcome) =
                            perform_operation(self.platform_specific.as_ref(), operation)
                        {
                            response.push(outcome);
                        }
                    }
                }

                if let Some(message_id) = message_id {
                    if !response.is_empty() {
                        self.pending_messages
                            .push((message_id, Ok(response.encode())));
                    }
                }
            }
            Ok(HardwareMessage::Malloc { size, alignment }) => {
                // TODO: this is obviously badly written
                let size = match usize::try_from(size) {
                    Ok(s) => s,
                    Err(_) => panic!(),
                };
                let buffer = Vec::with_capacity(size + usize::from(alignment) - 1);
                let mut ptr = match u64::try_from(buffer.as_ptr() as usize) {
                    Ok(p) => p,
                    Err(_) => panic!(),
                };
                while ptr % u64::from(alignment) != 0 {
                    ptr += 1;
                }

                let mut allocations = self.allocations.lock();
                allocations.entry(emitter_pid).or_default().push(buffer);

                if let Some(message_id) = message_id {
                    self.pending_messages.push((message_id, Ok(ptr.encode())));
                }
            }
            Ok(HardwareMessage::Free { ptr }) => {
                if let Ok(ptr) = usize::try_from(ptr) {
                    let mut allocations = self.allocations.lock();
                    if let Some(list) = allocations.get_mut(&emitter_pid) {
                        // Since we adjust the returned pointer to match the alignment.
                        list.retain(|e| {
                            ptr < e.as_ptr() as usize || ptr >= (e.as_ptr() as usize) + e.len()
                        });
                    }
                }
            }
            Ok(HardwareMessage::InterruptWait(_int_id)) => unimplemented!(), // TODO:
            Err(_) => {
                if let Some(message_id) = message_id {
                    self.pending_messages.push((message_id, Err(())))
                }
            }
        }
    }

    fn process_destroyed(self, pid: Pid) {
        self.allocations.lock().remove(&pid);
    }

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}

unsafe fn perform_operation<TPlat>(
    platform_specific: Pin<&TPlat>,
    operation: Operation,
) -> Option<HardwareAccessResponse>
where
    TPlat: PlatformSpecific,
{
    match operation {
        Operation::PhysicalMemoryMemset {
            address,
            len,
            value,
        } => {
            if let Ok(mut address) = usize::try_from(address) {
                for _ in 0..len {
                    (address as *mut u8).write_volatile(value);
                    if let Some(addr_next) = address.checked_add(1) {
                        address = addr_next;
                    } else {
                        break;
                    }
                }
            }
            None
        }
        Operation::PhysicalMemoryWriteU8 { address, data } => {
            if let Ok(mut address) = usize::try_from(address) {
                for byte in data {
                    (address as *mut u8).write_volatile(byte);
                    if let Some(addr_next) = address.checked_add(1) {
                        address = addr_next;
                    } else {
                        break;
                    }
                }
            }
            None
        }
        Operation::PhysicalMemoryWriteU16 { address, data } => {
            if let Ok(mut address) = usize::try_from(address) {
                for word in data {
                    (address as *mut u16).write_volatile(word);
                    if let Some(addr_next) = address.checked_add(2) {
                        address = addr_next;
                    } else {
                        break;
                    }
                }
            }
            None
        }
        Operation::PhysicalMemoryWriteU32 { address, data } => {
            if let Ok(mut address) = usize::try_from(address) {
                for dword in data {
                    (address as *mut u32).write_volatile(dword);
                    if let Some(addr_next) = address.checked_add(4) {
                        address = addr_next;
                    } else {
                        break;
                    }
                }
            }
            None
        }
        Operation::PhysicalMemoryReadU8 { address, len } => {
            // TODO: try allocate `len` but don't panic if `len` is too large
            let mut out = Vec::with_capacity(len as usize); // TODO: don't use `as`
            let mut address = Some(address);
            for _ in 0..len {
                if let Some(addr) = address {
                    out.push((addr as *mut u8).read_volatile());
                    address = addr.checked_add(1);
                } else {
                    out.push(0);
                }
            }
            Some(HardwareAccessResponse::PhysicalMemoryReadU8(out))
        }
        Operation::PhysicalMemoryReadU16 { address, len } => {
            // TODO: try allocate `len` but don't panic if `len` is too large
            let mut out = Vec::with_capacity(len as usize); // TODO: don't use `as`
            let mut address = Some(address);
            for _ in 0..len {
                if let Some(addr) = address {
                    out.push((addr as *mut u16).read_volatile());
                    address = addr.checked_add(2);
                } else {
                    out.push(0);
                }
            }
            Some(HardwareAccessResponse::PhysicalMemoryReadU16(out))
        }
        Operation::PhysicalMemoryReadU32 { address, len } => {
            // TODO: try allocate `len` but don't panic if `len` is too large
            let mut out = Vec::with_capacity(len as usize); // TODO: don't use `as`
            let mut address = Some(address);
            for _ in 0..len {
                if let Some(addr) = address {
                    out.push((addr as *mut u32).read_volatile());
                    address = addr.checked_add(4);
                } else {
                    out.push(0);
                }
            }
            Some(HardwareAccessResponse::PhysicalMemoryReadU32(out))
        }
        Operation::PortWriteU8 { port, data } => {
            let _ = platform_specific.write_port_u8(port, data);
            None
        }
        Operation::PortWriteU16 { port, data } => {
            let _ = platform_specific.write_port_u16(port, data);
            None
        }
        Operation::PortWriteU32 { port, data } => {
            let _ = platform_specific.write_port_u32(port, data);
            None
        }
        Operation::PortReadU8 { port } => Some(HardwareAccessResponse::PortReadU8(
            platform_specific.read_port_u8(port).unwrap_or(0),
        )),
        Operation::PortReadU16 { port } => Some(HardwareAccessResponse::PortReadU16(
            platform_specific.read_port_u16(port).unwrap_or(0),
        )),
        Operation::PortReadU32 { port } => Some(HardwareAccessResponse::PortReadU32(
            platform_specific.read_port_u32(port).unwrap_or(0),
        )),
    }
}
