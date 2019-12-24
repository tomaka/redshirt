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

//! Implements the time interface.

use futures::{channel::mpsc, lock::Mutex, prelude::*, stream::FuturesUnordered};
use futures_timer::Delay;
use parity_scale_codec::{DecodeAll, Encode as _};
use redshirt_core::Pid;
use redshirt_core::native::{NativeProgram, NativeProgramEvent, NativeProgramMessageIdwrite};
use redshirt_time_interface::ffi::{INTERFACE, TimeMessage};
use std::{
    convert::TryFrom,
    pin::Pin,
    sync::atomic,
    time::{Duration, Instant, SystemTime},
};

/// State machine for `time` interface messages handling.
pub struct TimerHandler<TMsgId> {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    /// Accessed only by `next_event`.
    inner: Mutex<TimerHandlerInner<TMsgId>>,
    /// Send on this channel the new timers to insert in [`TimerHandlerInner::timers`].
    new_timer_tx: mpsc::UnboundedSender<(Delay, TMsgId)>,
}

/// Separate struct behind a mutex.
struct TimerHandlerInner<TMsgId> {
    /// Stream of message IDs to answer.
    timers: FuturesUnordered<Pin<Box<dyn Future<Output = TMsgId> + Send>>>, // TODO: meh for boxing
    /// Receiving side of [`TimerHandler::new_timer_tx`].
    new_timer_rx: mpsc::UnboundedReceiver<(Delay, TMsgId)>,
}

impl<TMsgId> TimerHandler<TMsgId>
where
    TMsgId: Send + 'static,
{
    /// Initializes the new state machine for timers.
    pub fn new() -> Self {
        let (new_timer_tx, new_timer_rx) = mpsc::unbounded();

        TimerHandler {
            inner: Mutex::new(TimerHandlerInner {
                timers: {
                    let timers =
                        FuturesUnordered::<Pin<Box<dyn Future<Output = TMsgId> + Send>>>::new();
                    // TODO: ugh; pushing a never-ending future, otherwise we get a permanent `None` when polling
                    timers.push(Box::pin(async move {
                        loop {
                            futures::pending!()
                        }
                    }));
                    timers
                },
                new_timer_rx,
            }),
            new_timer_tx,
        }
    }

    /// Processes a message on the `time` interface, and optionally returns an answer to
    /// immediately send back.
    pub fn time_message(
        &self,
        message_id: Option<TMsgId>,
        message: &[u8],
    ) -> Option<Result<Vec<u8>, ()>> {
        match TimeMessage::decode_all(&message) {
            Ok(TimeMessage::GetMonotonic) => Some(Ok(monotonic_clock().encode())),
            Ok(TimeMessage::GetSystem) => Some(Ok(system_clock().encode())),
            Ok(TimeMessage::WaitMonotonic(until)) => match until.checked_sub(monotonic_clock()) {
                None => Some(Ok(().encode())),
                Some(dur_from_now) => {
                    // If `dur_from_now` is larger than a `u64`, we simply don't insert any timer.
                    // We assume that we will never reach this time ever.
                    if let Ok(dur) = u64::try_from(dur_from_now) {
                        let dur = Duration::from_nanos(dur);
                        self.new_timer_tx
                            .unbounded_send((Delay::new(dur), message_id.unwrap()))
                            .unwrap();
                    }
                    None
                }
            },
            Err(_) => Some(Err(())),
        }
    }

    /// Returns the next message to answer, and the message to send back.
    pub async fn next_answer(&self) -> (TMsgId, Vec<u8>) {
        let mut inner = self.inner.lock().await;
        let inner = &mut *inner;

        loop {
            match future::select(inner.timers.next(), inner.new_timer_rx.next()).await {
                future::Either::Left((Some(message_id), _)) => return (message_id, ().encode()),
                future::Either::Right((Some((new_delay, message_id)), _)) => {
                    inner.timers.push(Box::pin(async move {
                        new_delay.await;
                        message_id
                    }));
                }
                future::Either::Left((None, _)) => unreachable!(),
                future::Either::Right((None, _)) => unreachable!(),
            }
        }
    }
}

impl<'a> NativeProgram<'a> for TimerHandler {
    /// Future resolving to the next event the [`NativeProgram`] emits.
    type Future: Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(&'a self) -> Self::Future {
        async move {
            if !self.registered.swap(true, atomic::Ordering::Relaxed) {
                return NativeProgramEvent::Emit {
                    interface: INTERFACE,
                    message_id_write: None,
                    message: ,
                },
            }
        }
    }

    fn interface_message(
        &self,
        interface: [u8; 32],
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: Vec<u8>
    ) {
        debug_assert_eq!(interface, INTERFACE);

        match TimeMessage::decode_all(&message) {
            Ok(TimeMessage::GetMonotonic) => Some(Ok(monotonic_clock().encode())),
            Ok(TimeMessage::GetSystem) => Some(Ok(system_clock().encode())),
            Ok(TimeMessage::WaitMonotonic(until)) => match until.checked_sub(monotonic_clock()) {
                None => Some(Ok(().encode())),
                Some(dur_from_now) => {
                    // If `dur_from_now` is larger than a `u64`, we simply don't insert any timer.
                    // We assume that we will never reach this time ever.
                    if let Ok(dur) = u64::try_from(dur_from_now) {
                        let dur = Duration::from_nanos(dur);
                        self.new_timer_tx
                            .unbounded_send((Delay::new(dur), message_id.unwrap()))
                            .unwrap();
                    }
                    None
                }
            },
            Err(_) => Some(Err(())),
        }
    }

    fn process_destroyed(&self, _: Pid) {
    }

    fn message_response(&self, _: MessageId, _: Vec<u8>) {
        unreachable!()
    }
}

fn monotonic_clock() -> u128 {
    lazy_static::lazy_static! {
        static ref CLOCK_START: Instant = Instant::now();
    }
    let start = *CLOCK_START;
    duration_to_u128(start.elapsed())
}

fn system_clock() -> u128 {
    duration_to_u128(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap(),
    )
}

fn duration_to_u128(duration: Duration) -> u128 {
    u128::from(duration.as_secs() * 1_000_000_000) + u128::from(duration.subsec_nanos())
}
