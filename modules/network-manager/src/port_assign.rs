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

use hashbrown::HashSet;
use rand::distributions::{Distribution as _, Uniform};

/// Port assignment system. Keeps track of which port is used.
///
/// This struct doesn't know and doesn't care whether it is used by TCP, UDP, or something else.
/// It is expected that one instance of this struct exists for each protocol.
pub struct PortAssign {
    occupied: HashSet<u16, fnv::FnvBuildHasher>,
}

impl PortAssign {
    /// Builds a new [`PortAssign`] with no port assigned.
    pub fn new() -> PortAssign {
        PortAssign {
            occupied: Default::default(),
        }
    }

    /// Try to reserve a specific port. Returns an error if the port was already reserved.
    pub fn reserve(&mut self, port: u16) -> Result<(), ()> {
        if self.occupied.insert(port) {
            Ok(())
        } else {
            Err(())
        }
    }

    /// Reserves a port whose value is superior to `min`. Returns `None` if no port is available.
    pub fn reserve_any(&mut self, min: u16) -> Option<u16> {
        if (min..=u16::max_value()).all(|p| self.occupied.contains(&p)) {
            return None;
        }

        loop {
            let attempt =
                Uniform::new_inclusive(min, u16::max_value()).sample(&mut rand::thread_rng());
            if self.occupied.insert(attempt) {
                debug_assert!(attempt >= min);
                return Some(attempt);
            }
        }
    }

    /// Un-reserves a port. Returns an error if the port wasn't reserved.
    pub fn free(&mut self, port: u16) -> Result<(), ()> {
        if self.occupied.remove(&port) {
            Ok(())
        } else {
            Err(())
        }
    }
}
