// Copyright(c) 2019 Pierre Krieger

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

/// Pool of identifiers. Can assign new identifiers from it.
// TODO: since PID/ThreadIDs/MessageIDs are exposed in user space, make them unpredictable
pub struct IdPool {
    next: AtomicU64,
}

impl IdPool {
    /// Initializes a new pool.
    pub fn new() -> Self {
        IdPool {
            // TODO: randomness?
            next: AtomicU64::new(0),
        }
    }

    /// Assigns a new PID from this pool.
    pub fn assign<T: From<u64>>(&self) -> T {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if id == u64::max_value() {
            panic!() // TODO: ?
        }
        T::from(id)
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
            assert!(ids.insert(pool.assign()));
        }
    }
}
