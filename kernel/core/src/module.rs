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

use core::fmt;

/// Hash of a module.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ModuleHash([u8; 32]);

/// Error that can happen when calling [`ModuleHash::from_base58`].
#[derive(Debug)]
pub struct FromBase58Error {}

impl From<[u8; 32]> for ModuleHash {
    fn from(hash: [u8; 32]) -> ModuleHash {
        ModuleHash(hash)
    }
}

impl From<ModuleHash> for [u8; 32] {
    fn from(hash: ModuleHash) -> [u8; 32] {
        hash.0
    }
}

impl ModuleHash {
    /// Returns the hash of the given bytes.
    pub fn from_bytes(buffer: impl AsRef<[u8]>) -> Self {
        ModuleHash(blake3::hash(buffer.as_ref()).into())
    }

    /// Decodes the given base58-encoded string into a hash.
    ///
    /// See also https://en.wikipedia.org/wiki/Base58.
    // TODO: check that we return an error if the string is too long
    pub fn from_base58(encoding: &str) -> Result<Self, FromBase58Error> {
        let mut out = [0; 32];
        let written = bs58::decode(encoding)
            .into(&mut out)
            .map_err(|_| FromBase58Error {})?;
        let mut out2 = [0; 32];
        out2[32 - written..].copy_from_slice(&out[..written]);
        Ok(ModuleHash(out2))
    }
}

impl fmt::Debug for ModuleHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ModuleHash({})", bs58::encode(&self.0).into_string())
    }
}

impl fmt::Display for FromBase58Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FromBase58Error")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_wat_works() {
        let _ = from_wat!(local, "(module)");
    }

    #[test]
    fn simple_wat_works() {
        let _ = from_wat!(
            local,
            r#"
            (module
                (func $add (param i32 i32) (result i32)
                    local.get 0
                    local.get 1
                    i32.add)
                (export "add" (func $add)))
            "#
        );
    }
}
