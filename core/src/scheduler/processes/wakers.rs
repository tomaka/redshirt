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

use core::{fmt, task::Waker};
use slab::Slab;
use spinning_top::Spinlock;

/// Collection of wakers.
#[derive(Default)]
pub struct Wakers {
    list: Spinlock<Slab<Option<Waker>>>,
}

impl Wakers {
    pub fn register(&self) -> Registration {
        let mut list = self.list.lock();
        let index = list.insert(None);
        Registration {
            list: &self.list,
            index,
        }
    }

    /// Wakes up one registered waker. Has no effect if the list is empty.
    pub fn notify_one(&self) {
        let mut list = self.list.lock();
        for (_, elem) in list.iter_mut() {
            if let Some(elem) = elem.take() {
                elem.wake();
                return;
            }
        }
    }
}

impl fmt::Debug for Wakers {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Wakers").finish()
    }
}

pub struct Registration<'a> {
    list: &'a Spinlock<Slab<Option<Waker>>>,
    index: usize,
}

impl<'a> Registration<'a> {
    pub fn set_waker(&mut self, waker: &Waker) {
        let mut list = self.list.lock();
        let entry = &mut list[self.index];
        if let Some(entry) = entry {
            if entry.will_wake(waker) {
                return;
            }
        }
        *entry = Some(waker.clone());
    }
}

impl<'a> fmt::Debug for Registration<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Registration").finish()
    }
}

impl<'a> Drop for Registration<'a> {
    fn drop(&mut self) {
        self.list.lock().remove(self.index);
    }
}
