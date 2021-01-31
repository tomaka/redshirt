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

//! Native program that handles the `random` interface.

use crate::{arch::PlatformSpecific, random::rng::KernelRng};

use alloc::{sync::Arc, vec};
use core::pin::Pin;
use crossbeam_queue::SegQueue;
use rand_core::RngCore as _;
use redshirt_core::{Decode as _, EncodedMessage};
use redshirt_random_interface::ffi::RandomMessage;

/// State machine for `random` interface messages handling.
pub struct RandomNativeProgram {
    /// Queue of random number generators. If it is empty, we generate a new one.
    rngs: SegQueue<KernelRng>,
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<PlatformSpecific>>,
}

impl RandomNativeProgram {
    /// Initializes the new state machine for random messages handling.
    pub fn new(platform_specific: Pin<Arc<PlatformSpecific>>) -> Self {
        RandomNativeProgram {
            rngs: SegQueue::new(),
            platform_specific,
        }
    }

    /// Fills the given buffer with random bytes.
    pub fn fill_bytes(&self, out: &mut [u8]) {
        let mut rng = if let Some(rng) = self.rngs.pop() {
            rng
        } else {
            KernelRng::new(self.platform_specific.clone())
        };

        rng.fill_bytes(out);
        self.rngs.push(rng);
    }

    pub fn interface_message(&self, message: EncodedMessage) -> Result<EncodedMessage, ()> {
        match RandomMessage::decode(message) {
            Ok(RandomMessage::Generate { len }) => {
                let mut out = vec![0; usize::from(len)];
                self.fill_bytes(&mut out);
                Ok(EncodedMessage(out))
            }
            Err(_) => Err(()),
        }
    }
}
