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

use crate::monotonic_clock;
use std::{convert::TryFrom, ops::Add, ops::Sub, time::Duration};

/// Mimics the API of `std::time::Instant`, except that it works.
///
/// > **Note**: This struct should be removed in the future, as `std::time::Instant` should work
/// >           when compiling to WASI.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant {
    pub(crate) inner: u128,
}

impl Instant {
    pub fn now() -> Instant {
        let val = nametbd_syscalls_interface::block_on(monotonic_clock());
        Instant { inner: val }
    }

    pub fn duration_since(&self, earlier: Instant) -> Duration {
        *self - earlier
    }

    pub fn elapsed(&self) -> Duration {
        Instant::now() - *self
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;

    fn add(self, other: Duration) -> Instant {
        let new_val = self.inner + other.as_nanos();
        Instant { inner: new_val }
    }
}

impl Sub<Duration> for Instant {
    type Output = Instant;

    fn sub(self, other: Duration) -> Instant {
        let new_val = self.inner - other.as_nanos();
        Instant { inner: new_val }
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;

    fn sub(self, other: Instant) -> Duration {
        let ns = self.inner - other.inner;
        // TODO: not great to unwrap, but we don't care
        Duration::from_nanos(u64::try_from(ns).unwrap())
    }
}
