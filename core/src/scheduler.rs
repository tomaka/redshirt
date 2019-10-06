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
