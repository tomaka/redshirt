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

use core::time::Duration;

/// Returns the amount of time that has elapsed since an undeterminate moment in time.
#[cfg(target_arch = "x86_64")]
pub fn monotonic_clock() -> Duration {
    // TODO: wrong unit
    let ns = unsafe { core::arch::x86_64::_rdtsc() };
    Duration::from_nanos(ns)
}

/// Returns the amount of time that has elapsed since an undeterminate moment in time.
#[cfg(target_arch = "arm")]
pub fn monotonic_clock() -> Duration {
    // TODO: ugh
    // TODO: assumes that performance counters are supported and enabled
    let reg: u32;
    unsafe {
        asm!("mrc p15, 0, $0, c9, c13, 0" : "=r"(reg) ::: "volatile");
    }
    Duration::from_nanos(u64::from(reg))
}
