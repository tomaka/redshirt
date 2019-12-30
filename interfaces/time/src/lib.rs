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

//! Time.

#![deny(intra_doc_link_resolution_failure)]
#![no_std]

extern crate alloc;

use core::time::Duration;
use futures::prelude::*;

pub use self::delay::Delay;
pub use self::instant::Instant;

mod delay;
mod instant;

pub mod ffi;

/// Returns the number of nanoseconds since an arbitrary point in time in the past.
pub fn monotonic_clock() -> impl Future<Output = u128> {
    unsafe {
        let msg = ffi::TimeMessage::GetMonotonic;
        redshirt_syscalls_interface::emit_message_with_response(&ffi::INTERFACE, msg)
            .map(|v| v.unwrap())
    }
}

/// Returns the number of nanoseconds since the Epoch (January 1st, 1970 at midnight UTC).
pub fn system_clock() -> impl Future<Output = u128> {
    unsafe {
        let msg = ffi::TimeMessage::GetSystem;
        redshirt_syscalls_interface::emit_message_with_response(&ffi::INTERFACE, msg)
            .map(|v| v.unwrap())
    }
}

/// Returns a `Future` that yields when the monotonic clock reaches this value.
pub fn monotonic_wait_until(until: u128) -> impl Future<Output = ()> {
    unsafe {
        let msg = ffi::TimeMessage::WaitMonotonic(until);
        redshirt_syscalls_interface::emit_message_with_response(&ffi::INTERFACE, msg)
            .map(|v| v.unwrap())
    }
}

/// Returns a `Future` that outputs after `duration` has elapsed.
pub fn monotonic_wait(duration: Duration) -> impl Future<Output = ()> {
    let dur_nanos = u128::from(duration.as_secs())
        .saturating_mul(1_000_000_000)
        .saturating_add(u128::from(duration.subsec_nanos()));

    // TODO: meh for two syscalls
    monotonic_clock().then(move |now| monotonic_wait_until(now.saturating_add(dur_nanos)))
}
