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

use core::{convert::TryFrom, fmt, num::NonZeroU64};
use crossbeam_queue::SegQueue;
use rand::distributions::{Distribution as _, Uniform};
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng as _;
use rand_hc::Hc128Rng;
use spinning_top::Spinlock;

// Maths note: after 3 billion iterations, there's a 2% chance of a collision
//
// Chance of collision is approximately: 1 - exp(-n^2 / 2^(b+1))
// where `n` is the number of generated IDs, `b` number of bits in the ID (64 here)

/// Lock-free pool of identifiers. Can assign new identifiers from it.
pub struct IdPool {
    /// Sources of randomness.
    /// Every time we need a random number, we pop a state from this list, then push it back when
    /// we're done.
    rngs: SegQueue<ChaCha20Rng>,
    /// Distribution of IDs.
    distribution: Uniform<u64>,
    /// RNG to seed other RNGs. We use a different algorithm than ChaCha in order to not clone
    /// the ChaCha state when we derive from it.
    // TODO: is it actually needed to have a different algorithm, or is this comment bullshit?
    //       using a different algorithm doesn't hurt, but it'd be better if the comment was correct
    master_rng: Spinlock<Hc128Rng>,
}

impl IdPool {
    /// Initializes a new pool.
    pub fn new() -> Self {
        IdPool {
            rngs: SegQueue::new(),
            distribution: Uniform::from(0..=u64::max_value()),
            master_rng: Spinlock::new(Hc128Rng::from_seed([0; 32])), // FIXME: proper seed
        }
    }

    /// Assigns a new PID from this pool.
    ///
    /// The returned value must implement the `TryFrom<u64>` trait. `u64`s are rolled as long as
    /// as calling `TryFrom` returns an error.
    pub fn assign<T: TryFrom<u64>>(&self) -> T {
        if let Some(mut rng) = self.rngs.pop() {
            return loop {
                let raw_id = self.distribution.sample(&mut rng);
                if let Ok(id) = TryFrom::try_from(raw_id) {
                    self.rngs.push(rng);
                    break id;
                }
            };
        }

        let mut master_rng = self.master_rng.lock();
        let mut new_rng = match ChaCha20Rng::from_rng(&mut *master_rng) {
            Ok(r) => r,
            Err(_) => unreachable!(),
        };

        loop {
            let raw_id = self.distribution.sample(&mut new_rng);
            if let Ok(id) = TryFrom::try_from(raw_id) {
                self.rngs.push(new_rng);
                break id;
            }
        }
    }
}

impl fmt::Debug for IdPool {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("IdPool").finish()
    }
}

#[cfg(test)]
mod tests {
    use nohash_hasher::BuildNoHashHasher;

    #[test]
    fn ids_different() {
        let mut ids = hashbrown::HashSet::<u64, BuildNoHashHasher<u64>>::default();
        let pool = super::IdPool::new();
        for _ in 0..5000 {
            assert!(ids.insert(pool.assign()));
        }
    }
}
