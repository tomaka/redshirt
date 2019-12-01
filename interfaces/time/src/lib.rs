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
#![cfg_attr(not(feature = "std"), no_std)]

use core::time::Duration;

#[cfg(feature = "std")]
pub use self::delay::Delay;
#[cfg(feature = "std")]
pub use self::instant::Instant;

#[cfg(feature = "std")]
mod delay;
pub mod ffi;
#[cfg(feature = "std")]
mod instant;

/// Returns the number of nanoseconds since an arbitrary point in time in the past.
#[cfg(feature = "std")]
pub async fn monotonic_clock() -> u128 {
    let msg = ffi::TimeMessage::GetMonotonic;
    nametbd_syscalls_interface::emit_message_with_response(ffi::INTERFACE, msg)
        .await
        .unwrap()
}

/// Returns the number of nanoseconds since the Epoch (January 1st, 1970 at midnight UTC).
#[cfg(feature = "std")]
pub async fn system_clock() -> u128 {
    let msg = ffi::TimeMessage::GetSystem;
    nametbd_syscalls_interface::emit_message_with_response(ffi::INTERFACE, msg)
        .await
        .unwrap()
}

/// Returns a `Future` that yields when the monotonic clock reaches this value.
#[cfg(feature = "std")]
pub async fn monotonic_wait_until(until: u128) {
    let msg = ffi::TimeMessage::WaitMonotonic(until);
    nametbd_syscalls_interface::emit_message_with_response(ffi::INTERFACE, msg)
        .await
        .unwrap()
}

/// Returns a `Future` that outputs after `duration` has elapsed.
#[cfg(feature = "std")]
pub async fn monotonic_wait(duration: Duration) {
    let dur_nanos = u128::from(duration.as_secs())
        .saturating_mul(1_000_000_000)
        .saturating_add(u128::from(duration.subsec_nanos()));

    // TODO: meh for two syscalls
    let now = monotonic_clock().await;
    monotonic_wait_until(now.saturating_add(dur_nanos)).await;
}
