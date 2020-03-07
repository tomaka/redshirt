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

use std::{io, path::Path, process::Command};

/// Turn an ELF file into a binary.
// TODO: define exact semantics of what this function does on the file
pub fn elf_to_binary(src: impl AsRef<Path>, dest: impl AsRef<Path>) -> Result<(), io::Error> {
    // TODO: implement this in pure Rust?
    let status = Command::new("objcopy") // TODO: can be a different objcopy
        .args(&["-O", "binary"])
        .arg(src.as_ref())
        .arg(dest.as_ref())
        .status()?;
    // TODO: make it configurable where stdout/stderr go?
    if !status.success() {
        return Err(io::Error::from(io::ErrorKind::Other));
    }

    Ok(())
}
