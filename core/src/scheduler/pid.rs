// Copyright(c) 2019 Pierre Krieger

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Identifier of a running process within a core.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Pid(u64);

/// Pool of identifiers. Can assign new identifiers from it.
// TODO: since PID are exposed in user space, make them unpredictable
pub struct PidPool {
    next: AtomicU64,
}

impl fmt::Debug for Pid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl PidPool {
    /// Initializes a new pool.
    pub fn new() -> Self {
        PidPool {
            // TODO: randomness?
            next: AtomicU64::new(0),
        }
    }

    /// Assigns a new PID from this pool.
    pub fn assign(&self) -> Pid {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if id == u64::max_value() {
            panic!() // TODO: ?
        }
        Pid(id)
    }
}

impl From<Pid> for u64 {
    fn from(pid: Pid) -> u64 {
        pid.0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_different() {
        let mut ids = hashbrown::HashSet::new();
        let pool = super::PidPool::new();
        for _ in 0..5000 {
            assert!(ids.insert(pool.assign()));
        }
    }
}
