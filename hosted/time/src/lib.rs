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

use nametbd_time_interface::ffi::TimeMessage;
use parity_scale_codec::{DecodeAll, Encode as _};
use std::time::{Duration, Instant, SystemTime};

/// Processes a message on the `time` interface, and returns the answer send to back.
pub fn time_message(message: &[u8]) -> Vec<u8> {
    match TimeMessage::decode_all(&message).unwrap() {
        // TODO: don't unwrap
        TimeMessage::GetMonotonic => monotonic_clock().encode(),
        TimeMessage::GetSystem => system_clock().encode(),
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
