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

use crate::{monotonic_wait_until, Instant};
use alloc::boxed::Box;
use core::{fmt, future::Future, pin::Pin, task::Context, task::Poll, time::Duration};

/// Mimics the API of `futures_timer::Delay`.
pub struct Delay {
    when: Instant,
    inner: Pin<Box<dyn Future<Output = ()> + Send>>,
}

impl Delay {
    pub fn new(dur: Duration) -> Delay {
        Delay::new_at(Instant::now() + dur)
    }

    pub fn new_at(at: Instant) -> Delay {
        Delay {
            when: at,
            inner: Box::pin(monotonic_wait_until(at.inner)),
        }
    }

    pub fn when(&self) -> Instant {
        self.when
    }

    pub fn reset(&mut self, at: Instant) {
        *self = Delay::new_at(at);
    }
}

impl fmt::Debug for Delay {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Delay").field("when", &self.when).finish()
    }
}

impl Future for Delay {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        Future::poll(self.inner.as_mut(), cx)
    }
}
