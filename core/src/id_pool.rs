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

use core::fmt;
use crossbeam::queue::SegQueue;
use rand_chacha::{ChaCha20Core, ChaCha20Rng};
use rand_core::SeedableRng as _;
use rand_distr::{Distribution as _, Uniform};

/// Lock-free pool of identifiers. Can assign new identifiers from it.
pub struct IdPool {
    /// Queue of RNG objects. Since generating a value requires an exclusive reference to the
    /// RNG object, we hold a queue of objects.
    rngs_queue: SegQueue<ChaCha20Rng>,
    /// Distribution of IDs.
    distribution: Uniform<u64>,
}

impl IdPool {
    /// Initializes a new pool.
    pub fn new() -> Self {
        IdPool {
            rngs_queue: SegQueue::new(),
            distribution: Uniform::from(0..=u64::max_value()),
        }
    }

    /// Assigns a new PID from this pool.
    pub fn assign<T: From<u64>>(&self) -> T {
        let mut rng = self.rngs_queue.pop().unwrap_or_else(|_| self.gen_new_rng());
        let id = self.distribution.sample(&mut rng);
        self.rngs_queue.push(rng);
        T::from(id)
    }

    /// Generates a new `ChaCha20Rng`.
    fn gen_new_rng(&self) -> ChaCha20Rng {
        let core = ChaCha20Core::from_seed([0; 32]);        // TODO:
        core.into()
    }
}

impl fmt::Debug for IdPool {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("IdPool").finish()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_different() {
        let mut ids = hashbrown::HashSet::<u64>::new();
        let pool = super::IdPool::new();
        for _ in 0..5000 {
            // TODO: since it's random, there's a small chance that this fails?
            assert!(ids.insert(pool.assign()));
        }
    }
}
