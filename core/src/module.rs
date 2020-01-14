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

/// Represents a successfully-parsed binary.
///
/// This is the equivalent of an [ELF](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format)
/// or a [PE](https://en.wikipedia.org/wiki/Portable_Executable).
pub struct Module {
    inner: wasmi::Module,
    hash: ModuleHash,
}

/// Hash of a module.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ModuleHash([u8; 32]);

/// Error that can happen when calling `from_bytes`.
#[derive(Debug)]
pub struct FromBytesError {}

impl Module {
    /// Parses a module from WASM bytes.
    pub fn from_bytes(buffer: impl AsRef<[u8]>) -> Result<Self, FromBytesError> {
        let inner = wasmi::Module::from_buffer(buffer.as_ref()).map_err(|_| FromBytesError {})?;
        let hash = ModuleHash::from_bytes(buffer);

        Ok(Module { inner, hash })
    }

    /// Turns some WASM text source into a `Module`.
    #[cfg(test)] // TODO: is `#[cfg(test)]` a good idea?
    pub fn from_wat(source: impl AsRef<[u8]>) -> Result<Self, wat::Error> {
        let wasm = wat::parse_bytes(source.as_ref())?;
        Ok(Self::from_bytes(wasm).unwrap())
    }

    /// Returns a reference to the internal module.
    pub(crate) fn as_ref(&self) -> &wasmi::Module {
        &self.inner
    }

    /// Returns the hash of that module.
    ///
    /// This gives the same result as calling `ModuleHash::from_bytes` on the original input.
    pub fn hash(&self) -> &ModuleHash {
        &self.hash
    }
}

impl From<[u8; 32]> for ModuleHash {
    fn from(hash: [u8; 32]) -> ModuleHash {
        ModuleHash(hash)
    }
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Module({})", bs58::encode(&self.hash.0).into_string())
    }
}

impl ModuleHash {
    /// Returns the hash of the given bytes.
    pub fn from_bytes(buffer: impl AsRef<[u8]>) -> Self {
        ModuleHash(blake3::hash(buffer.as_ref()).into())
    }
}

impl fmt::Debug for ModuleHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ModuleHash({})", bs58::encode(&self.0).into_string())
    }
}

impl fmt::Display for FromBytesError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FromBytesError")
    }
}

#[cfg(test)]
mod tests {
    use super::Module;

    #[test]
    fn empty_wat_works() {
        let _ = Module::from_wat("(module)").unwrap();
    }

    #[test]
    fn simple_wat_works() {
        let _ = Module::from_wat(
            r#"
            (module
                (func $add (param i32 i32) (result i32)
                    get_local 0
                    get_local 1
                    i32.add)
                (export "add" (func $add)))
            "#,
        )
        .unwrap();
    }
}
