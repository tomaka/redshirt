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

pub use nametbd_time_interface::Instant;

use std::{fmt, future::Future, io, pin::Pin, task::Context, task::Poll, time::Duration};

/// Mimics the API of `futures_timer::Delay`.
#[pin_project::pin_project]
pub struct Delay {
    #[pin]
    inner: nametbd_time_interface::Delay,
}

impl Delay {
    pub fn new(dur: Duration) -> Delay {
        Delay {
            inner: nametbd_time_interface::Delay::new(dur),
        }
    }

    pub fn new_at(at: Instant) -> Delay {
        Delay {
            inner: nametbd_time_interface::Delay::new_at(at),
        }
    }

    pub fn when(&self) -> Instant {
        self.inner.when()
    }

    pub fn reset(&mut self, dur: Duration) {
        *self = Delay::new(dur);
    }

    pub fn reset_at(&mut self, at: Instant) {
        *self = Delay::new_at(at);
    }
}

impl fmt::Debug for Delay {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl Future for Delay {
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut this = self.project();
        Future::poll(this.inner.as_mut(), cx).map(Ok)
    }
}
