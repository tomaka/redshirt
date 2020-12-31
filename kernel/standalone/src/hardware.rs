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

use crate::{arch::PlatformSpecific, future_channel};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{convert::TryFrom as _, num::NonZeroU64, pin::Pin, task::Poll};
use futures::prelude::*;
use hashbrown::HashMap;
use nohash_hasher::BuildNoHashHasher;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, MessageId, Pid};
use redshirt_hardware_interface::ffi::{
    HardwareAccessResponse, HardwareMessage, Operation, INTERFACE,
};
use spinning_top::Spinlock;

/// State machine for `hardware` interface messages handling.
pub struct HardwareHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::Atomic<bool>,
    /// If `Some`, contains the registration ID towards the `interface` interface.
    registration_id: atomic::Atomic<Option<NonZeroU64>>,
    /// Number of message requests that need to be emitted.
    pending_message_requests: atomic::Atomic<u8>,
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<PlatformSpecific>>,
    /// For each PID, a list of memory allocations.
    // TODO: optimize
    allocations: Spinlock<HashMap<Pid, Vec<Vec<u8>>, BuildNoHashHasher<u64>>>,
    /// Sending side of `pending_messages`.
    pending_messages_tx: future_channel::UnboundedSender<(MessageId, Result<EncodedMessage, ()>)>,
    /// List of messages waiting to be emitted with `next_event`.
    pending_messages: future_channel::UnboundedReceiver<(MessageId, Result<EncodedMessage, ()>)>,
}

impl HardwareHandler {
    /// Initializes the new state machine for hardware accesses.
    pub fn new(platform_specific: Pin<Arc<PlatformSpecific>>) -> Self {
        let (pending_messages_tx, pending_messages) = future_channel::channel();
        HardwareHandler {
            registered: atomic::Atomic::new(false),
            registration_id: atomic::Atomic::new(None),
            pending_message_requests: atomic::Atomic::new(16),
            platform_specific,
            allocations: Spinlock::new(HashMap::default()),
            pending_messages_tx,
            pending_messages,
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
                    message_id_write: Some(DummyMessageIdWrite),
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        INTERFACE,
                    )
                    .encode(),
                };
            }

            if let Some(registration_id) = self.registration_id.load(atomic::Ordering::Relaxed) {
                loop {
                    let v = self
                        .pending_message_requests
                        .load(atomic::Ordering::Relaxed);
                    if v == 0 {
                        break;
                    }
                    if self
                        .pending_message_requests
                        .compare_exchange(
                            v,
                            v - 1,
                            atomic::Ordering::Relaxed,
                            atomic::Ordering::Relaxed,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    return NativeProgramEvent::Emit {
                        interface: redshirt_interface_interface::ffi::INTERFACE,
                        message_id_write: Some(DummyMessageIdWrite),
                        message: redshirt_interface_interface::ffi::InterfaceMessage::NextMessage(
                            registration_id,
                        )
                        .encode(),
                    };
                }
            }

            future::poll_fn(move |cx| {
                if let Poll::Ready((message_id, answer)) = self.pending_messages.poll_next(cx) {
                    return Poll::Ready(NativeProgramEvent::Emit {
                        interface: redshirt_interface_interface::ffi::INTERFACE,
                        message_id_write: None,
                        message: redshirt_interface_interface::ffi::InterfaceMessage::Answer(
                            message_id,
                            answer.map(|m| m.0),
                        )
                        .encode(),
                    });
                }

                Poll::Pending
            })
            .await
        })
    }

    fn message_response(self, _: MessageId, response: Result<EncodedMessage, ()>) {
        debug_assert!(self.registered.load(atomic::Ordering::Relaxed));

        // The first ever message response that can be received is the interface registration.
        if self
            .registration_id
            .load(atomic::Ordering::Relaxed)
            .is_none()
        {
            let registration_id =
                match redshirt_interface_interface::ffi::InterfaceRegisterResponse::decode(
                    response.unwrap(),
                )
                .unwrap()
                .result
                {
                    Ok(id) => id,
                    // A registration error means the interface has already been registered. Returning
                    // here stalls this state machine forever.
                    Err(_) => return,
                };

            self.registration_id
                .store(Some(registration_id), atomic::Ordering::Relaxed);
            return;
        }

        // If this is reached, the response is a response to a message request.
        self.pending_message_requests
            .fetch_add(1, atomic::Ordering::Relaxed);

        let notification =
            match redshirt_interface_interface::ffi::decode_notification(&response.unwrap().0)
                .unwrap()
            {
                redshirt_interface_interface::DecodedInterfaceOrDestroyed::Interface(n) => n,
                redshirt_interface_interface::DecodedInterfaceOrDestroyed::ProcessDestroyed(n) => {
                    self.allocations.lock().remove(&n.pid);
                    return;
                }
            };

        match HardwareMessage::decode(notification.actual_data) {
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

                if let Some(message_id) = notification.message_id {
                    if !response.is_empty() {
                        self.pending_messages_tx
                            .unbounded_send((message_id, Ok(response.encode())));
                    }
                }
            }
            Ok(HardwareMessage::Malloc { size, alignment }) => {
                // TODO: this is obviously badly written
                let size = match usize::try_from(size) {
                    Ok(s) => s,
                    Err(_) => panic!(),
                };
                let align = match usize::try_from(alignment) {
                    Ok(s) => s,
                    Err(_) => panic!(),
                };
                let buffer = Vec::with_capacity(size + align - 1);
                let mut ptr = match u64::try_from(buffer.as_ptr() as usize) {
                    Ok(p) => p,
                    Err(_) => panic!(),
                };
                while ptr % alignment != 0 {
                    ptr += 1;
                }

                let mut allocations = self.allocations.lock();
                allocations
                    .entry(notification.emitter_pid)
                    .or_default()
                    .push(buffer);

                if let Some(message_id) = notification.message_id {
                    self.pending_messages_tx
                        .unbounded_send((message_id, Ok(ptr.encode())));
                }
            }
            Ok(HardwareMessage::Free { ptr }) => {
                if let Ok(ptr) = usize::try_from(ptr) {
                    let mut allocations = self.allocations.lock();
                    if let Some(list) = allocations.get_mut(&notification.emitter_pid) {
                        // Since we adjust the returned pointer to match the alignment.
                        list.retain(|e| {
                            ptr < e.as_ptr() as usize || ptr >= (e.as_ptr() as usize) + e.len()
                        });
                    }
                }
            }
            Ok(HardwareMessage::InterruptWait(_int_id)) => unimplemented!(), // TODO:
            Err(_) => {
                if let Some(message_id) = notification.message_id {
                    self.pending_messages_tx
                        .unbounded_send((message_id, Err(())))
                }
            }
        }
    }
}

unsafe fn perform_operation(
    platform_specific: Pin<&PlatformSpecific>,
    operation: Operation,
) -> Option<HardwareAccessResponse>
where
{
    match operation {
        Operation::PhysicalMemoryMemset {
            address,
            len,
            value,
        } => {
            if let Ok(mut address) = usize::try_from(address) {
                for _ in 0..len {
                    if address != 0 {
                        (address as *mut u8).write_volatile(value);
                    }
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
                    if address != 0 {
                        (address as *mut u8).write_volatile(byte);
                    }
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
                    if address != 0 {
                        (address as *mut u16).write_volatile(word);
                    }
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
                    if address != 0 {
                        (address as *mut u32).write_volatile(dword);
                    }
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
                    if addr == 0 {
                        out.push(0);
                    } else {
                        out.push((addr as *mut u8).read_volatile());
                    }
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
                    if addr == 0 {
                        out.push(0);
                    } else {
                        out.push((addr as *mut u16).read_volatile());
                    }
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
                    if addr == 0 {
                        out.push(0);
                    } else {
                        out.push((addr as *mut u32).read_volatile());
                    }
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
