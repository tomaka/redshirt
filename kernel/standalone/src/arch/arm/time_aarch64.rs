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

#![cfg(target_arch = "aarch64")]

//! This module is a draft.
// TODO: implement properly

use alloc::sync::Arc;
use core::{
    arch::asm,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub struct TimeControl {}

pub struct TimerFuture {}

impl TimeControl {
    pub unsafe fn init() -> Arc<TimeControl> {
        Arc::new(TimeControl {})
    }

    pub fn monotonic_clock(self: &Arc<Self>) -> u128 {
        unsafe {
            // TODO: stub
            let val: u64;
            asm!("mrs {}, CNTPCT_EL0", out(reg) val, options(nostack, nomem, preserves_flags));
            u128::from(val)
        }
    }

    pub fn timer(self: &Arc<Self>, deadline: u128) -> TimerFuture {
        TimerFuture {}
    }
}

impl Future for TimerFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        Poll::Pending
    }
}
