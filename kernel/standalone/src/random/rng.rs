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

use rand_chacha::{ChaCha20Core, ChaCha20Rng};
use rand_core::{RngCore, SeedableRng as _};
use rand_jitter::JitterRng;
use sha2::{
    digest::{FixedOutput as _, Input as _},
    Sha512Trunc256,
};

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
            let mut rng = JitterRng::new_with_timer(|| {
                let dur = crate::time::monotonic_clock();
                dur.as_secs()
                    .wrapping_mul(1_000_000_000)
                    .wrapping_add(u64::from(dur.subsec_nanos()))
            });

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
            let mut sha2 = Sha512Trunc256::default();
            let mut jitter_bytes = [0; 64];
            jitter.fill_bytes(&mut jitter_bytes);
            sha2.input(&jitter_bytes[..]);
            add_hardware_entropy(&mut sha2);
            let mut chacha_seed = [0; 32];
            chacha_seed.copy_from_slice(&sha2.fixed_result());
            chacha_seed
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

#[cfg(target_arch = "x86_64")]
fn add_hardware_entropy(sha2: &mut Sha512Trunc256) {
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
        sha2.input(&buf[..entropy_bytes]);
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn add_hardware_entropy(_: &mut Sha512Trunc256) {}
