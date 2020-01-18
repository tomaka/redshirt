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

#[cfg(target_arch = "wasm32")]
pub use redshirt_time_interface::Instant;
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;

use std::{fmt, future::Future, io, pin::Pin, task::Context, task::Poll, time::Duration};

/// Mimics the API of `futures_timer::Delay`.
#[pin_project::pin_project]
pub struct Delay {
    #[cfg(not(target_arch = "wasm32"))]
    #[pin]
    inner: futures_timer::Delay,
    #[cfg(target_arch = "wasm32")]
    #[pin]
    inner: redshirt_time_interface::Delay,
}

#[cfg(target_arch = "wasm32")]
impl Delay {
    pub fn new(dur: Duration) -> Delay {
        Delay {
            inner: redshirt_time_interface::Delay::new(dur),
        }
    }

    pub fn new_at(at: Instant) -> Delay {
        Delay {
            inner: redshirt_time_interface::Delay::new_at(at),
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

#[cfg(not(target_arch = "wasm32"))]
impl Delay {
    pub fn new(dur: Duration) -> Delay {
        Delay {
            inner: futures_timer::Delay::new(dur),
        }
    }

    pub fn new_at(at: Instant) -> Delay {
        Delay {
            inner: futures_timer::Delay::new({
                let now = Instant::now();
                if at > now {
                    at - now
                } else {
                    Duration::new(0, 0)
                }
            }),
        }
    }

    pub fn when(&self) -> Instant {
        self.inner.when()
    }

    pub fn reset(&mut self, dur: Duration) {
        *self = Delay::new(dur);
    }

    pub fn reset_at(&mut self, at: Instant) {
        self.reset({
            let now = Instant::now();
            if at > now {
                at - now
            } else {
                Duration::new(0, 0)
            }
        });
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
