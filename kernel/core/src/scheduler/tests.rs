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

#![cfg(test)]

use crate::extrinsics::NoExtrinsics;

mod basic_module;
mod emit_not_available;
mod trapping_module;

#[test]
fn send_sync() {
    fn is_send_sync<T: Send + Sync>() {}
    is_send_sync::<super::Core<NoExtrinsics>>()
}
