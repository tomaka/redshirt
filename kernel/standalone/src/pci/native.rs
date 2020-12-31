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

use crate::{arch::PlatformSpecific, future_channel, pci::pci};

use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec::Vec};
use core::{convert::TryFrom as _, num::NonZeroU64, pin::Pin, task::Poll};
use futures::prelude::*;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, MessageId, Pid};
use redshirt_pci_interface::ffi;
use spinning_top::Spinlock;

/// State machine for `pci` interface messages handling.
pub struct PciNativeProgram {
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<PlatformSpecific>>,
    /// Future triggered the next time a PCI device generates an interrupt.
    // TODO: at the moment we don't differentiate between devices
    next_irq: Spinlock<Pin<Box<dyn Future<Output = ()> + Send>>>,

    /// Devices manager. Does the actual work.
    devices: pci::PciDevices,
    /// List of devices locked by processes.
    locked_devices: Spinlock<Vec<LockedDevice>>,

    /// If true, we have sent the interface registration message.
    registered: atomic::Atomic<bool>,
    /// If `Some`, contains the registration ID towards the `interface` interface.
    registration_id: atomic::Atomic<Option<NonZeroU64>>,
    /// Number of message requests that need to be emitted.
    pending_message_requests: atomic::Atomic<u8>,
    /// Sending side of `pending_messages`.
    pending_messages_tx: future_channel::UnboundedSender<(MessageId, Result<EncodedMessage, ()>)>,
    /// List of messages waiting to be emitted with `next_event`.
    pending_messages: future_channel::UnboundedReceiver<(MessageId, Result<EncodedMessage, ()>)>,
}

#[derive(Debug)]
struct LockedDevice {
    owner: Pid,
    bdf: ffi::PciDeviceBdf,

    /// List of `MessageId`s sent and requesting to be answered when the next interrupt happens.
    next_interrupt_messages: VecDeque<MessageId>,
}

impl PciNativeProgram {
    /// Initializes the new state machine for PCI messages handling.
    pub fn new(devices: pci::PciDevices, platform_specific: Pin<Arc<PlatformSpecific>>) -> Self {
        let next_irq = Spinlock::new(Box::pin(PlatformSpecific::next_irq(
            platform_specific.as_ref(),
        )) as Pin<Box<_>>);

        let (pending_messages_tx, pending_messages) = future_channel::channel();

        PciNativeProgram {
            platform_specific,
            next_irq,
            devices,
            locked_devices: Spinlock::new(Vec::new()),
            registered: atomic::Atomic::new(false),
            registration_id: atomic::Atomic::new(None),
            pending_message_requests: atomic::Atomic::new(16),
            pending_messages_tx,
            pending_messages,
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a PciNativeProgram {
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        Box::pin(async move {
            // Register ourselves as the PCI interface provider, if not already done.
            if !self.registered.swap(true, atomic::Ordering::Relaxed) {
                return NativeProgramEvent::Emit {
                    interface: redshirt_interface_interface::ffi::INTERFACE,
                    message_id_write: Some(DummyMessageIdWrite),
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        ffi::INTERFACE,
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

            loop {
                // Wait either for next IRQ or next pending message.
                let ev = future::poll_fn(move |cx| {
                    if let Poll::Ready(()) = Future::poll(Pin::new(&mut *self.next_irq.lock()), cx)
                    {
                        return Poll::Ready(None);
                    }

                    if let Poll::Ready((message_id, answer)) = self.pending_messages.poll_next(cx) {
                        return Poll::Ready(Some(NativeProgramEvent::Emit {
                            interface: redshirt_interface_interface::ffi::INTERFACE,
                            message_id_write: None,
                            message: redshirt_interface_interface::ffi::InterfaceMessage::Answer(
                                message_id,
                                answer.map(|m| m.0),
                            )
                            .encode(),
                        }));
                    }

                    Poll::Pending
                })
                .await;

                // Message received on `pending_messages`.
                if let Some(ev) = ev {
                    return ev;
                }

                // We reach here only if an IRQ happened.

                // We grab the next IRQ future now, in order to not miss any IRQ happening
                // while `locked_devices` is processed below.
                *self.next_irq.lock() =
                    Box::pin(PlatformSpecific::next_irq(self.platform_specific.as_ref()))
                        as Pin<Box<_>>;

                // Wake up all the devices.
                let mut locked_devices = self.locked_devices.lock();
                for device in locked_devices.iter_mut() {
                    for msg in device.next_interrupt_messages.drain(..) {
                        let answer =
                            redshirt_pci_interface::ffi::NextInterruptResponse::Interrupt.encode();
                        self.pending_messages_tx.unbounded_send((msg, Ok(answer)));
                    }
                }
            }
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
                    self.locked_devices.lock().retain(|dev| dev.owner != n.pid);
                    return;
                }
            };

        match ffi::PciMessage::decode(notification.actual_data) {
            Ok(ffi::PciMessage::LockDevice(bdf)) => {
                let mut locked_devices = self.locked_devices.lock();
                if locked_devices.iter().any(|dev| dev.bdf == bdf) {
                    if let Some(message_id) = notification.message_id {
                        self.pending_messages_tx
                            .unbounded_send((message_id, Ok(Result::<(), _>::Err(()).encode())));
                    }
                } else {
                    // TODO: check device validity
                    locked_devices.push(LockedDevice {
                        owner: notification.emitter_pid,
                        bdf,
                        next_interrupt_messages: VecDeque::new(),
                    });

                    if let Some(message_id) = notification.message_id {
                        self.pending_messages_tx
                            .unbounded_send((message_id, Ok(Result::<_, ()>::Ok(()).encode())));
                    }
                }
            }

            Ok(ffi::PciMessage::UnlockDevice(bdf)) => {
                let emitter_pid = notification.emitter_pid;
                let mut locked_devices = self.locked_devices.lock();
                if let Some(pos) = locked_devices
                    .iter_mut()
                    .position(|dev| dev.owner == emitter_pid && dev.bdf == bdf)
                {
                    let locked_device = locked_devices.remove(pos);
                    for m in locked_device.next_interrupt_messages {
                        self.pending_messages_tx
                            .unbounded_send((m, Ok(ffi::NextInterruptResponse::Unlocked.encode())));
                    }
                }
            }

            Ok(ffi::PciMessage::SetCommand {
                location,
                io_space,
                memory_space,
                bus_master,
            }) => {
                let emitter_pid = notification.emitter_pid;
                let locked_devices = self.locked_devices.lock();
                if locked_devices
                    .iter()
                    .any(|dev| dev.owner == emitter_pid && dev.bdf == location)
                {
                    self.devices
                        .devices()
                        .find(|d| {
                            d.bus() == location.bus
                                && d.device() == location.device
                                && d.function() == location.function
                        })
                        .unwrap()
                        .set_command(bus_master, memory_space, io_space);
                }
            }

            Ok(ffi::PciMessage::NextInterrupt(bdf)) => {
                // TODO: actually make these interrupts work
                if let Some(message_id) = notification.message_id {
                    let emitter_pid = notification.emitter_pid;
                    let mut locked_devices = self.locked_devices.lock();
                    if let Some(dev) = locked_devices
                        .iter_mut()
                        .find(|dev| dev.owner == emitter_pid && dev.bdf == bdf)
                    {
                        dev.next_interrupt_messages.push_back(message_id);
                    } else {
                        self.pending_messages_tx.unbounded_send((
                            message_id,
                            Ok(ffi::NextInterruptResponse::BadDevice.encode()),
                        ));
                    }
                }
            }

            Ok(ffi::PciMessage::GetDevicesList) => {
                if let Some(message_id) = notification.message_id {
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
                                            base_address: u64::try_from(base_address).unwrap(),
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

                    self.pending_messages_tx
                        .unbounded_send((message_id, Ok(response.encode())));
                }
            }

            Ok(_) => unimplemented!(),

            Err(_) => {
                if let Some(message_id) = notification.message_id {
                    self.pending_messages_tx
                        .unbounded_send((message_id, Err(())))
                }
            }
        }
    }
}
