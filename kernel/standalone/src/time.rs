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

//! Implements the `time` interface.

use crate::arch::PlatformSpecific;

use alloc::{boxed::Box, sync::Arc};
use core::pin::Pin;
use futures::{prelude::*, stream::FuturesUnordered, task::Poll};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, MessageId};
use redshirt_time_interface::ffi::TimeMessage;
use spinning_top::Spinlock;

/// State machine for `time` interface messages handling.
pub struct TimeHandler {
    /// Platform-specific hooks.
    platform_specific: Pin<Arc<PlatformSpecific>>,
    /// List of active timers.
    timers: Spinlock<FuturesUnordered<Pin<Box<dyn Future<Output = MessageId> + Send>>>>,
}

impl TimeHandler {
    /// Initializes the new state machine for time accesses.
    pub fn new(platform_specific: Pin<Arc<PlatformSpecific>>) -> Self {
        TimeHandler {
            platform_specific,
            timers: Spinlock::new(FuturesUnordered::new()),
        }
    }

    pub fn interface_message(
        &self,
        message_id: MessageId,
        message: EncodedMessage,
    ) -> Option<Result<EncodedMessage, ()>> {
        match TimeMessage::decode(message) {
            Ok(TimeMessage::GetMonotonic) => {
                let now = self.platform_specific.as_ref().monotonic_clock();
                Some(Ok(now.encode()))
            }
            Ok(TimeMessage::WaitMonotonic(value)) => {
                let timers = self.timers.lock();
                timers.push(
                    self.platform_specific
                        .as_ref()
                        .timer(value)
                        .map(move |_| message_id)
                        .boxed(),
                );
                None
            }
            Err(_) => Some(Err(())),
        }
    }

    pub async fn next_response(&self) -> (MessageId, EncodedMessage) {
        future::poll_fn(move |cx| {
            let mut timers = self.timers.lock();
            if timers.is_empty() {
                return Poll::Pending;
            }

            let message_id = match timers.poll_next_unpin(cx) {
                Poll::Ready(Some(id)) => id,
                Poll::Ready(None) => unreachable!(),
                Poll::Pending => return Poll::Pending,
            };

            Poll::Ready((message_id, ().encode()))
        })
        .await
    }
}
