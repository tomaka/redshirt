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

//! Random number generation.
//!
//! This module aims to provide cryptographically-secure random number generation.
//!
//! Since computers are deterministic, it is surprisingly difficult to generate entropy. This is
//! typically not a concern for most developers in the world, because most of the time when
//! a program needs random data, it simply asks the kernel for some (e.g. by reading
//! `/dev/urandom` on Unix). Here, however, we *are* the kernel.
//!
//! In order to generate entropy, we can rely on:
//!
//! - Hardware random number generators, such as `rdrand` on x86/x64. This is however generally
//! widely untrusted.
//! - Unpredictable events coming from the hardware, such as time between keyboard presses or
//! network packets.
//! - CPU execution time jitter. The time it takes for a CPU to execute instructions is very hard
//! to predict because of caches, memory bus speed, power management, and so on.
//!
//! # Implementation in redshirt
//!
//! The current implementation relies on ChaCha20 seeded by a JitterRng and RdRand if it is
//! available.
//!

// TODO: I'm not a cryptographer nor a mathematician, but I guess that a ChaCha alone is a bit naive?

use core::convert::TryFrom as _;
use rand_chacha::{ChaCha20Core, ChaCha20Rng};
use rand_core::{RngCore, SeedableRng as _};
use rand_jitter::JitterRng;

/// Kernel random number generator.
pub struct KernelRng {
    /// Inner PRNG.
    rng: ChaCha20Rng,
}

impl KernelRng {
    /// Initializes a new [`KernelRng`].
    pub fn new() -> KernelRng {
        // Initialize the `JitterRng`.
        let mut jitter = {
            let mut rng = JitterRng::new_with_timer(timer);

            // This makes sure that the `JitterRng` is good enough. A panic here indicates that
            // our entropy would be too low.
            let rounds = match rng.test_timer() {
                Ok(r) => r,
                Err(err) => panic!("{:?}", err),
            };
            rng.set_rounds(rounds);
            // According to the documentation, we have to discard the first `u64`.
            let _ = rng.next_u64();
            rng
        };

        // We now build the seed for the main ChaCha PRNG.
        let chacha_seed = {
            let mut hasher = blake3::Hasher::new();
            let mut jitter_bytes = [0; 64];
            jitter.fill_bytes(&mut jitter_bytes);
            hasher.update(&jitter_bytes[..]);
            add_hardware_entropy(&mut hasher);
            let mut chacha_seed = [0; 32];
            <[u8; 32]>::from(hasher.finalize())
        };

        KernelRng {
            rng: From::from(ChaCha20Core::from_seed(chacha_seed)),
        }
    }
}

impl RngCore for KernelRng {
    fn next_u32(&mut self) -> u32 {
        self.rng.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.rng.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.rng.fill_bytes(dest)
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.rng.try_fill_bytes(dest)
    }
}

// TODO: because `JitterRng::new_with_timer` requires a function pointer and not a closure, we
// can't pass a `PlatformSpecific` trait impl, and instead have to use platform-specific code here

#[cfg(target_arch = "x86_64")]
fn add_hardware_entropy(hasher: &mut blake3::Hasher) {
    use byteorder::{ByteOrder as _, NativeEndian};

    if let Some(rdrand) = x86_64::instructions::random::RdRand::new() {
        let mut buf = [0; 64];
        let mut entropy_bytes = 0;
        for chunk in buf.chunks_mut(8) {
            if let Some(val) = rdrand.get_u64() {
                NativeEndian::write_u64(chunk, val);
                entropy_bytes += 8;
            } else {
                break;
            }
        }
        hasher.update(&buf[..entropy_bytes]);
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn add_hardware_entropy(_: &mut blake3::Hasher) {}

// Note: timer must have nanosecond precision, according to the documentation of `JitterRng`.
#[cfg(target_arch = "x86_64")]
fn timer() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}
#[cfg(not(target_arch = "x86_64"))]
fn timer() -> u64 {
    unsafe {
        let lo: u32;
        let hi: u32;
        // TODO: what about CNTFRQ? which code configures it? initial value is unknown at reset
        // Reading the CNTPCT register.
        asm!("mrrc p15, 0, $0, $1, c14": "=r"(lo), "=r"(hi) ::: "volatile");
        u64::from(hi) << 32 | u64::from(lo)
    }
}
