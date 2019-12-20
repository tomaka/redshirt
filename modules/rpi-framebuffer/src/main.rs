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

//! Implements the stdout interface by writing in text mode.

// TODO: doc https://jsandler18.github.io/

use byteorder::{ByteOrder as _, LittleEndian};
use parity_scale_codec::DecodeAll;
use std::{convert::TryFrom as _, fmt};

mod mailbox;
mod property;

fn main() {
    redshirt_syscalls_interface::block_on(async_main());
}

async fn async_main()  {
    property::init().await;
}

