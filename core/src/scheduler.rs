// Copyright(c) 2019 Pierre Krieger

use core::fmt;

mod ipc;
mod processes;
mod vm;

// TODO: move definition?
pub use self::ipc::{Core, CoreBuilder, CoreProcess, CoreRunOutcome};
pub use self::processes::ThreadId;

/// Identifier of a running process within a core.
// TODO: move to a Pid module?
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Pid(u64);

impl From<u64> for Pid {
    fn from(id: u64) -> Pid {
        Pid(id)
    }
}

impl From<Pid> for u64 {
    fn from(pid: Pid) -> u64 {
        pid.0
    }
}

impl fmt::Debug for Pid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}
