// Copyright (C) 2019-2020  Pierre Krieger
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

//! Synchronization of the TSC between multiple CPUs.
//!
//! The TSC (Time Stamp Counter) is a counter that is automatically incremented by the CPU either
//! at each clock cycle (for older CPUs), or at a constant rate (for more recent CPUs).
//!
//! It is the primary mechanism that can be used in order to approximate how much time has passed.
//!
//! However, each CPU has a different TSC, which means that a value read on one CPU has no meaning
//! on another one.
//!
//! In order to remedy this, whenever we start a new processor we synchronize its TSC to the one
//! of the startup CPUs.
//!
//! Keep in mind, however, that this synchronization is not perfect. It is possible to read the
//! TSC on a CPU, move this value to another CPU, and observe the value being lesser than the TSC
//! of the new CPU. Code that uses the TSC must be aware of that and cannot assume that the TSC is
//! strictly monotonic.
//!
//! # Usage
//!
//! Call [`tsc_sync`] to obtain a "sender" and a "receiver". Call [`TscSyncSrc::sync`] and
//! [`TscSyncDst::sync`] from two different CPUs. The TSC of the CPU that has called
//! [`TscSyncDst::sync`] will have its TSC synchronized with the one of the CPU that has called
//! [`TscSyncSrc::sync`].
//!

// Implementation strategy:
//
// Before `sync` actually starts, we use a barrier to ensure that the two CPUs start executing
// `sync` at the same time.
//
// Then, for a short period of time, the source repeatedly writes its TSC value in a shared
// variable, which is continuously read by the destination. The destination repeatedly compares
// the value it reads to its own value.
//
// At the end of the measurement, the destination CPU knows the maximum difference between its own
// value and the source value, and updates its own value accordingly.

use alloc::sync::Arc;
use core::sync::atomic;
use crossbeam_utils::CachePadded;
use spinning_top::Spinlock;
use x86_64::registers::model_specific::Msr;

/// Returns a "sender" and a "receiver". Call [`TscSyncSrc::sync`] and [`TscSyncDst::sync`] from
/// two different CPUs, so that the TSC of the CPU that called [`TscSyncDst::sync`] synchronizes
/// with the TSC of the CPU that called [`TscSyncSrc::sync`].
pub fn tsc_sync() -> (TscSyncSrc, TscSyncDst) {
    let shared = Arc::new(CachePadded::from(Shared {
        start_barrier: atomic::AtomicU8::new(0),
        src_rdtsc_storage: Spinlock::new(0),
    }));

    let src = TscSyncSrc {
        shared: shared.clone(),
    };
    let dst = TscSyncDst { shared };
    (src, dst)
}

/// Use on the CPU with the reference clock to sync from.
pub struct TscSyncSrc {
    shared: Arc<CachePadded<Shared>>,
}

/// Use on the CPU to be synced.
pub struct TscSyncDst {
    shared: Arc<CachePadded<Shared>>,
}

struct Shared {
    /// Number of CPUs (0, 1, or 2) that have entered `sync`.
    /// Note that this should be a barrier, but there isn't any spinlock-based barrier in the
    /// Rust ecosystem at the time of writing this code.
    start_barrier: atomic::AtomicU8,

    /// Mutex where the source CPU will repeatedly store its TSC value. A value of 0 means
    /// "hasn't been written yet" and should be ignored.
    src_rdtsc_storage: Spinlock<u64>,
}

impl TscSyncSrc {
    /// Perform the synchronization.
    ///
    /// > **Important**: This will wait for the corresponding [`TscSyncDst`] to call
    /// >                [`sync`](`TscSyncDst::sync`).
    pub fn sync(&mut self) {
        assert!(Arc::strong_count(&self.shared) >= 2);

        // Barrier to synchronize the source and destination.
        self.shared
            .start_barrier
            .fetch_add(1, atomic::Ordering::SeqCst);
        while self.shared.start_barrier.load(atomic::Ordering::SeqCst) != 2 {
            atomic::spin_loop_hint();
        }

        for _ in 0..100000 {
            let mut lock = self.shared.src_rdtsc_storage.lock();
            *lock = volatile_rdtsc();
        }
    }
}

impl TscSyncDst {
    /// Perform the synchronization.
    ///
    /// > **Important**: This will wait for the corresponding [`TscSyncSrc`] to call
    /// >                [`sync`](`TscSyncSrc::sync`).
    pub fn sync(&mut self) {
        assert!(Arc::strong_count(&self.shared) >= 2);

        // Barrier to synchronize the source and destination.
        self.shared
            .start_barrier
            .fetch_add(1, atomic::Ordering::SeqCst);
        while self.shared.start_barrier.load(atomic::Ordering::SeqCst) != 2 {
            atomic::spin_loop_hint();
        }

        // Maximum value for `local - remote`
        let mut max_over = 0;
        // Maximum value for `remote - local`
        let mut max_under = 0;

        for _ in 0..100000 {
            let lock = self.shared.src_rdtsc_storage.lock();
            let local_value = volatile_rdtsc();
            let remote_value = *lock;
            drop(lock);

            if remote_value == 0 {
                continue;
            }

            if let Some(over) = local_value.checked_sub(remote_value) {
                if over > max_over {
                    max_over = over;
                }
            }
            if let Some(under) = remote_value.checked_sub(local_value) {
                if under > max_under {
                    max_under = under;
                }
            }
        }

        // FIXME: we just assume that the "ADJUST" register is supported; this is wrong

        const TSC_ADJUST_REGISTER: Msr = Msr::new(0x3b);
        let current_adjust = unsafe { TSC_ADJUST_REGISTER.read() };
        let new_adjust = if max_over > max_under {
            current_adjust.wrapping_sub(max_over)
        } else {
            current_adjust.wrapping_add(max_under)
        };
        unsafe { TSC_ADJUST_REGISTER.write(new_adjust) }
    }
}

/// Reads the TSC value of the current CPU while trying to force the CPU to not re-order the
/// execution of the `rdtsc` opcode.
// TODO: preferably use `rdtscp` instead if supported, which has this property by default
fn volatile_rdtsc() -> u64 {
    #[cfg(target_arch = "x86")]
    fn inner() -> u64 {
        unsafe {
            core::arch::x86::_mm_lfence();
            core::arch::x86::_rdtsc()
        }
    }
    #[cfg(target_arch = "x86_64")]
    fn inner() -> u64 {
        unsafe {
            core::arch::x86_64::_mm_lfence();
            core::arch::x86_64::_rdtsc()
        }
    }
    inner()
}
