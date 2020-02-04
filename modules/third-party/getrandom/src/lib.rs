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

//! Override of the `getrandom` crate.

mod error;
pub use crate::error::Error;

pub fn getrandom(dest: &mut [u8]) -> Result<(), error::Error> {
    if dest.is_empty() {
        return Ok(());
    }

    redshirt_syscalls::block_on(async move {
        redshirt_random_interface::generate_in(dest).await;
    });

    Ok(())
}
