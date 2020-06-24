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

//! Native program that handles the `pci` interface.

use crate::{arch::PlatformSpecific, pci::pci};

use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec, vec::Vec};
use core::{convert::TryFrom as _, pin::Pin, sync::atomic};
use crossbeam_queue::SegQueue;
use futures::prelude::*;
use rand_core::RngCore as _;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_pci_interface::ffi;
use spinning_top::Spinlock;

/// State machine for `pci` interface messages handling.
pub struct PciNativeProgram<TPlat> {
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<TPlat>>,
    /// Future triggered the next time a PCI device generates an interrupt.
    // TODO: at the moment we don't differentiate between devices
    next_irq: Spinlock<Pin<Box<dyn Future<Output = ()> + Send>>>,

    /// Devices manager. Does the actual work.
    devices: pci::PciDevices,
    /// List of devices locked by processes.
    locked_devices: Spinlock<Vec<LockedDevice>>,

    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// Message responses waiting to be emitted.
    // TODO: must notify the next_event future
    pending_messages: SegQueue<(MessageId, Result<EncodedMessage, ()>)>,
}

#[derive(Debug)]
struct LockedDevice {
    owner: Pid,
    bdf: ffi::PciDeviceBdf,

    /// List of `MessageId`s sent and requesting to be answered when the next interrupt happens.
    next_interrupt_messages: VecDeque<MessageId>,
}

impl<TPlat> PciNativeProgram<TPlat>
where
    TPlat: PlatformSpecific,
{
    /// Initializes the new state machine for PCI messages handling.
    pub fn new(devices: pci::PciDevices, platform_specific: Pin<Arc<TPlat>>) -> Self {
        let next_irq =
            Spinlock::new(Box::pin(TPlat::next_irq(platform_specific.as_ref())) as Pin<Box<_>>);

        PciNativeProgram {
            platform_specific,
            next_irq,
            devices,
            locked_devices: Spinlock::new(Vec::new()),
            registered: atomic::AtomicBool::new(false),
            pending_messages: SegQueue::new(),
        }
    }
}

impl<'a, TPlat> NativeProgramRef<'a> for &'a PciNativeProgram<TPlat>
where
    TPlat: PlatformSpecific,
{
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        // Register ourselves as the PCI interface provider, if not already done.
        if !self.registered.swap(true, atomic::Ordering::Relaxed) {
            return Box::pin(future::ready(NativeProgramEvent::Emit {
                interface: redshirt_interface_interface::ffi::INTERFACE,
                message_id_write: None,
                message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                    ffi::INTERFACE,
                )
                .encode(),
            }));
        }

        if let Ok((message_id, answer)) = self.pending_messages.pop() {
            Box::pin(future::ready(NativeProgramEvent::Answer {
                message_id,
                answer,
            }))
        } else {
            let next_irq = &self.next_irq;
            let locked_devices = &self.locked_devices;
            let platform_specific = &self.platform_specific;

            Box::pin(async move {
                loop {
                    if let Ok((message_id, answer)) = self.pending_messages.pop() {
                        return NativeProgramEvent::Answer { message_id, answer };
                    }

                    // Wait for next IRQ.
                    future::poll_fn(move |cx| Future::poll(Pin::new(&mut *next_irq.lock()), cx))
                        .await;

                    // We grab the next IRQ future now, in order to not miss any IRQ happening
                    // while `locked_devices` is processed below.
                    *next_irq.lock() =
                        Box::pin(TPlat::next_irq(platform_specific.as_ref())) as Pin<Box<_>>;

                    let mut locked_devices = locked_devices.lock();
                    for device in locked_devices.iter_mut() {
                        for msg in device.next_interrupt_messages.drain(..) {
                            let answer =
                                redshirt_pci_interface::ffi::NextInterruptResponse::Interrupt
                                    .encode();
                            self.pending_messages.push((msg, Ok(answer)));
                        }
                    }
                    drop(locked_devices);
                }
            })
        }
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        debug_assert_eq!(interface, ffi::INTERFACE);

        match ffi::PciMessage::decode(message) {
            Ok(ffi::PciMessage::LockDevice(bdf)) => {
                let mut locked_devices = self.locked_devices.lock();
                if locked_devices.iter().any(|dev| dev.bdf == bdf) {
                    if let Some(message_id) = message_id {
                        self.pending_messages
                            .push((message_id, Ok(Result::<(), _>::Err(()).encode())));
                    }
                } else {
                    // TODO: check device validity
                    locked_devices.push(LockedDevice {
                        owner: emitter_pid,
                        bdf,
                        next_interrupt_messages: VecDeque::new(),
                    });

                    if let Some(message_id) = message_id {
                        self.pending_messages
                            .push((message_id, Ok(Result::<_, ()>::Ok(()).encode())));
                    }
                }
            }

            Ok(ffi::PciMessage::UnlockDevice(bdf)) => {
                let mut locked_devices = self.locked_devices.lock();
                if let Some(pos) = locked_devices
                    .iter_mut()
                    .position(|dev| dev.owner == emitter_pid && dev.bdf == bdf)
                {
                    let locked_device = locked_devices.remove(pos);
                    for m in locked_device.next_interrupt_messages {
                        self.pending_messages
                            .push((m, Ok(ffi::NextInterruptResponse::Unlocked.encode())));
                    }
                }
            }

            Ok(ffi::PciMessage::NextInterrupt(bdf)) => {
                // TODO: actually make these interrupts work
                if let Some(message_id) = message_id {
                    let mut locked_devices = self.locked_devices.lock();
                    if let Some(dev) = locked_devices
                        .iter_mut()
                        .find(|dev| dev.owner == emitter_pid && dev.bdf == bdf)
                    {
                        dev.next_interrupt_messages.push_back(message_id);
                    } else {
                        self.pending_messages.push((
                            message_id,
                            Ok(ffi::NextInterruptResponse::BadDevice.encode()),
                        ));
                    }
                }
            }

            Ok(ffi::PciMessage::GetDevicesList) => {
                if let Some(message_id) = message_id {
                    let response = ffi::GetDevicesListResponse {
                        devices: self
                            .devices
                            .devices()
                            .map(|device| ffi::PciDeviceInfo {
                                location: ffi::PciDeviceBdf {
                                    bus: device.bus(),
                                    device: device.device(),
                                    function: device.function(),
                                },
                                vendor_id: device.vendor_id(),
                                device_id: device.device_id(),
                                class_code: device.class_code(),
                                subclass: device.subclass(),
                                prog_if: device.prog_if(),
                                revision_id: device.revision_id(),
                                base_address_registers: device
                                    .base_address_registers()
                                    .map(|bar| match bar {
                                        pci::BaseAddressRegister::Memory {
                                            base_address, ..
                                        } => ffi::PciBaseAddressRegister::Memory {
                                            base_address: u32::try_from(base_address).unwrap(),
                                        },
                                        pci::BaseAddressRegister::Io { base_address } => {
                                            ffi::PciBaseAddressRegister::Io {
                                                base_address: u32::from(base_address),
                                            }
                                        }
                                    })
                                    .collect(),
                            })
                            .collect(),
                    };

                    self.pending_messages
                        .push((message_id, Ok(response.encode())));
                }
            }

            Ok(_) => unimplemented!(),

            Err(_) => {
                if let Some(message_id) = message_id {
                    self.pending_messages.push((message_id, Err(())))
                }
            }
        }
    }

    fn process_destroyed(self, pid: Pid) {
        self.locked_devices.lock().retain(|dev| dev.owner != pid);
    }

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}
