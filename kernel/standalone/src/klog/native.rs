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

//! Native program that handles the `kernel_log` interface.

// TODO: the `kernel_log` interface doesn't actually exist yet

use crate::klog::KLogger;

use alloc::{boxed::Box, sync::Arc, vec};
use core::{pin::Pin, sync::atomic};
use crossbeam_queue::SegQueue;
use futures::prelude::*;
use rand_core::RngCore as _;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_random_interface::ffi::{GenerateResponse, RandomMessage, INTERFACE};

/// State machine for `random` interface messages handling.
pub struct KernelLogNativeProgram<TPlat> {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// Object to configure.
    klogger: Arc<KLogger>,
}

impl<TPlat> KernelLogNativeProgram<TPlat> {
    /// Initializes the native program.
    pub fn new(klogger: Arc<KLogger>) -> Self {
        KernelLogNativeProgram {
            registered: atomic::AtomicBool::new(false),
            rngs: SegQueue::new(),
            pending_messages: SegQueue::new(),
            platform_specific,
        }
    }
}

impl<'a, TPlat> NativeProgramRef<'a> for &'a KernelLogNativeProgram<TPlat>
where
    TPlat: PlatformSpecific,
{
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        /*if !self.registered.swap(true, atomic::Ordering::Relaxed) {
            return Box::pin(future::ready(NativeProgramEvent::Emit {
                interface: redshirt_interface_interface::ffi::INTERFACE,
                message_id_write: None,
                message: redshirt_interface_interface::ffi::InterfaceMessage::Register(INTERFACE)
                    .encode(),
            }));
        }

        if let Ok((message_id, answer)) = self.pending_messages.pop() {
            Box::pin(future::ready(NativeProgramEvent::Answer {
                message_id,
                answer,
            }))
        } else {
            Box::pin(future::pending())
        }*/
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        message_id: Option<MessageId>,
        _emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        debug_assert_eq!(interface, INTERFACE);

        let message_id = match message_id {
            Some(m) => m,
            None => return,
        };

        match RandomMessage::decode(message) {
            Ok(RandomMessage::Generate { len }) => {
                let mut out = vec![0; usize::from(len)];

                let mut rng = if let Ok(rng) = self.rngs.pop() {
                    rng
                } else {
                    KernelRng::new(self.platform_specific.clone())
                };

                rng.fill_bytes(&mut out);
                self.rngs.push(rng);
                let response = GenerateResponse { result: out };
                self.pending_messages
                    .push((message_id, Ok(response.encode())));
            }
            Err(_) => self.pending_messages.push((message_id, Err(()))),
        }
    }

    fn process_destroyed(self, _: Pid) {}

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}
