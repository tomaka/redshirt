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

use crate::arch;

use alloc::{boxed::Box, vec::Vec};
use core::{convert::TryFrom as _, marker::PhantomData, pin::Pin, sync::atomic};
use futures::{prelude::*, channel::mpsc, lock::Mutex as FuturesMutex};
use hashbrown::HashMap;
use parity_scale_codec::{DecodeAll, Encode as _};
use redshirt_core::{MessageId, Pid};
use redshirt_core::native::{
    DummyMessageIdWrite, NativeProgramEvent, NativeProgramMessageIdWrite, NativeProgramRef,
};
use redshirt_hardware_interface::ffi::{INTERFACE, HardwareAccessResponse, HardwareMessage, Operation};
use spin::Mutex;

/// State machine for `hardware` interface messages handling.
pub struct HardwareHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// For each PID, a list of memory allocations.
    // TODO: optimize
    // TODO: free the list when a process gets destroyed (e.g. if it crashes)
    allocations: Mutex<HashMap<Pid, Vec<Vec<u8>>>>,
    messages_tx: mpsc::UnboundedSender<(MessageId, Result<Vec<u8>, ()>)>,
    messages_rx: FuturesMutex<mpsc::UnboundedReceiver<(MessageId, Result<Vec<u8>, ()>)>>,
}

impl HardwareHandler {
    /// Initializes the new state machine for hardware accesses.
    pub fn new() -> Self {
        let (messages_tx, messages_rx) = mpsc::unbounded();

        HardwareHandler {
            registered: atomic::AtomicBool::new(false),
            allocations: Mutex::new(HashMap::new()),
            messages_tx,
            messages_rx: FuturesMutex::new(messages_rx),
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a HardwareHandler {
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        Box::pin(async move {
            if !self.registered.swap(true, atomic::Ordering::Relaxed) {
                return NativeProgramEvent::Emit {
                    interface: redshirt_interface_interface::ffi::INTERFACE,
                    message_id_write: None,
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        INTERFACE,
                    )
                    .encode(),
                };
            }

            let mut messages_rx = self.messages_rx.lock().await;
            let (message_id, answer) = messages_rx.next().await.unwrap();
            return NativeProgramEvent::Answer {
                message_id,
                answer,
            };
        })
    }

    fn interface_message(
        self,
        interface: [u8; 32],
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: Vec<u8>,
    ) {
        debug_assert_eq!(interface, INTERFACE);

        match HardwareMessage::decode_all(&message) {
            Ok(HardwareMessage::HardwareAccess(operations)) => {
                let mut response = Vec::with_capacity(operations.len());
                for operation in operations {
                    unsafe {
                        if let Some(outcome) = perform_operation(operation) {
                            response.push(outcome);
                        }
                    }
                }

                if !response.is_empty() {
                    self.messages_tx.unbounded_send((message_id.unwrap(), Ok(response.encode()))).unwrap();
                }
            }
            Ok(HardwareMessage::Malloc { size, alignment }) => {
                // TODO: this is obviously badly written
                let mut buffer =
                    Vec::with_capacity(usize::try_from(size).unwrap() + usize::from(alignment) - 1);
                let mut ptr = u64::try_from(buffer.as_ptr() as usize).unwrap();
                while ptr % u64::from(alignment) != 0 {
                    ptr += 1;
                }

                let mut allocations = self.allocations.lock();
                allocations.entry(emitter_pid).or_default().push(buffer);

                self.messages_tx.unbounded_send((message_id.unwrap(), Ok(ptr.encode()))).unwrap();
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
            Ok(HardwareMessage::InterruptWait(int_id)) => unimplemented!(), // TODO:
            Err(_) => self.messages_tx.unbounded_send((message_id.unwrap(), Err(()))).unwrap(),
        }
    }

    fn process_destroyed(self, pid: Pid) {
        self.allocations.lock().remove(&pid);
    }

    fn message_response(self, _: MessageId, _: Vec<u8>) {
        unreachable!()
    }
}

unsafe fn perform_operation(operation: Operation) -> Option<HardwareAccessResponse> {
    match operation {
        Operation::PhysicalMemoryWriteU8 { address, data } => {
            if let Ok(mut address) = usize::try_from(address) {
                for byte in data {
                    (address as *mut u8).write_volatile(byte);
                    address = address.checked_add(1).unwrap();
                }
            }
            None
        }
        Operation::PhysicalMemoryWriteU16 { address, data } => {
            if let Ok(mut address) = usize::try_from(address) {
                for word in data {
                    (address as *mut u16).write_volatile(word);
                    address = address.checked_add(2).unwrap();
                }
            }
            None
        }
        Operation::PhysicalMemoryWriteU32 { address, data } => {
            if let Ok(mut address) = usize::try_from(address) {
                for dword in data {
                    (address as *mut u32).write_volatile(dword);
                    address = address.checked_add(4).unwrap();
                }
            }
            None
        }
        Operation::PhysicalMemoryReadU8 { mut address, len } => {
            // TODO: try allocate `len` but don't panic if `len` is too large
            let mut out = Vec::with_capacity(len as usize); // TODO: don't use `as`
            for _ in 0..len {
                out.push((address as *mut u8).read_volatile());
                address = address.checked_add(1).unwrap();
            }
            Some(HardwareAccessResponse::PhysicalMemoryReadU8(out))
        }
        Operation::PhysicalMemoryReadU16 { mut address, len } => {
            // TODO: try allocate `len` but don't panic if `len` is too large
            let mut out = Vec::with_capacity(len as usize); // TODO: don't use `as`
            for _ in 0..len {
                out.push((address as *mut u16).read_volatile());
                address = address.checked_add(2).unwrap();
            }
            Some(HardwareAccessResponse::PhysicalMemoryReadU16(out))
        }
        Operation::PhysicalMemoryReadU32 { mut address, len } => {
            // TODO: try allocate `len` but don't panic if `len` is too large
            let mut out = Vec::with_capacity(len as usize); // TODO: don't use `as`
            for _ in 0..len {
                out.push((address as *mut u32).read_volatile());
                address = address.checked_add(4).unwrap();
            }
            Some(HardwareAccessResponse::PhysicalMemoryReadU32(out))
        }
        Operation::PortWriteU8 { port, data } => {
            arch::write_port_u8(port, data);
            None
        }
        Operation::PortWriteU16 { port, data } => {
            arch::write_port_u16(port, data);
            None
        }
        Operation::PortWriteU32 { port, data } => {
            arch::write_port_u32(port, data);
            None
        }
        Operation::PortReadU8 { port } => {
            Some(HardwareAccessResponse::PortReadU8(arch::read_port_u8(port)))
        }
        Operation::PortReadU16 { port } => Some(HardwareAccessResponse::PortReadU16(
            arch::read_port_u16(port),
        )),
        Operation::PortReadU32 { port } => Some(HardwareAccessResponse::PortReadU32(
            arch::read_port_u32(port),
        )),
    }
}
