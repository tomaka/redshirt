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

//! Native program that handles the `pci` interface.

use crate::{arch::PlatformSpecific, pci::pci};

use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec::Vec};
use core::{convert::TryFrom as _, pin::Pin, task::Poll};
use crossbeam_queue::SegQueue;
use futures::prelude::*;
use redshirt_core::{
    extrinsics::Extrinsics, system::NativeInterfaceMessage, Decode as _, Encode as _,
    EncodedMessage, MessageId, Pid,
};
use redshirt_pci_interface::ffi;
use spinning_top::Spinlock;

/// State machine for `pci` interface messages handling.
pub struct PciNativeProgram {
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<PlatformSpecific>>,
    /// Future triggered the next time a PCI device generates an interrupt.
    // TODO: at the moment we don't differentiate between devices
    next_irq: Spinlock<Pin<Box<dyn Future<Output = ()> + Send>>>,

    pending_answers: SegQueue<(MessageId, EncodedMessage)>,

    /// Devices manager. Does the actual work.
    devices: pci::PciDevices,
    /// List of devices locked by processes.
    locked_devices: Spinlock<Vec<LockedDevice>>,
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

        PciNativeProgram {
            platform_specific,
            next_irq,
            pending_answers: crossbeam_queue::SegQueue::new(),
            devices,
            locked_devices: Spinlock::new(Vec::new()),
        }
    }

    pub async fn next_response(&self) -> (MessageId, EncodedMessage) {
        loop {
            if let Some(answer) = self.pending_answers.pop() {
                return answer;
            }

            future::poll_fn(move |cx| {
                let mut next_irq = self.next_irq.lock();
                match next_irq.poll_unpin(cx) {
                    Poll::Ready(()) => {}
                    Poll::Pending => return Poll::Pending,
                };

                // We grab the next IRQ future now, in order to not miss any IRQ happening
                // while `locked_devices` is processed below.
                *next_irq = Box::pin(PlatformSpecific::next_irq(self.platform_specific.as_ref()))
                    as Pin<Box<_>>;
                drop(next_irq);

                // Wake up all the devices.
                let mut locked_devices = self.locked_devices.lock();
                for device in locked_devices.iter_mut() {
                    for msg in device.next_interrupt_messages.drain(..) {
                        let answer =
                            redshirt_pci_interface::ffi::NextInterruptResponse::Interrupt.encode();
                        self.pending_answers.push((msg, answer));
                    }
                }

                Poll::Ready(())
            })
            .await
        }
    }

    pub fn interface_message<TExtr: Extrinsics>(
        &self,
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: NativeInterfaceMessage<TExtr>,
    ) -> Option<Result<EncodedMessage, ()>> {
        // Locking `locked_devices` ahead of time to avoid messages being processed in the
        // wrong order in case `interface_message` is called multiple times from different
        // threads.
        let mut locked_devices = self.locked_devices.lock();

        match ffi::PciMessage::decode(message.extract()) {
            Ok(ffi::PciMessage::LockDevice(bdf)) => {
                if locked_devices.iter().any(|dev| dev.bdf == bdf) {
                    Some(Ok(Result::<(), _>::Err(()).encode()))
                } else {
                    // TODO: check device validity
                    locked_devices.push(LockedDevice {
                        owner: emitter_pid,
                        bdf,
                        next_interrupt_messages: VecDeque::new(),
                    });

                    Some(Ok(Result::<_, ()>::Ok(()).encode()))
                }
            }

            Ok(ffi::PciMessage::UnlockDevice(bdf)) => {
                if let Some(pos) = locked_devices
                    .iter_mut()
                    .position(|dev| dev.owner == emitter_pid && dev.bdf == bdf)
                {
                    let locked_device = locked_devices.remove(pos);
                    for m in locked_device.next_interrupt_messages {
                        self.pending_answers
                            .push((m, ffi::NextInterruptResponse::Unlocked.encode()));
                    }
                }
                None
            }

            Ok(ffi::PciMessage::SetCommand {
                location,
                io_space,
                memory_space,
                bus_master,
            }) => {
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
                None
            }

            Ok(ffi::PciMessage::NextInterrupt(bdf)) => {
                // TODO: actually make these interrupts work
                if let Some(message_id) = message_id {
                    if let Some(dev) = locked_devices
                        .iter_mut()
                        .find(|dev| dev.owner == emitter_pid && dev.bdf == bdf)
                    {
                        dev.next_interrupt_messages.push_back(message_id);
                        None
                    } else {
                        Some(Ok(ffi::NextInterruptResponse::BadDevice.encode()))
                    }
                } else {
                    None
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

                    Some(Ok(response.encode()))
                } else {
                    None
                }
            }

            Ok(_) => unimplemented!(), // TODO:

            Err(_) => Some(Err(())),
        }
    }
}
