// Copyright(c) 2019 Pierre Krieger

use sha2::Digest as _;
use std::fmt;

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

impl Module {
    /// Parses a module from WASM bytes.
    // TODO: rename
    pub fn from_bytes(buffer: impl AsRef<[u8]>) -> Self {
        let inner = wasmi::Module::from_buffer(buffer.as_ref()).unwrap(); // TODO: don't unwrap
        let hash = ModuleHash::from_bytes(buffer);

        Module { inner, hash }
    }

    /// Turns some WASM text source into a `Module`.
    pub fn from_wat(source: impl AsRef<[u8]>) -> Result<Self, wabt::Error> {
        let wasm = wabt::wat2wasm(source)?;
        Ok(Self::from_bytes(wasm))
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
        ModuleHash(sha2::Sha256::digest(buffer.as_ref()).into())
    }
}

impl fmt::Debug for ModuleHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ModuleHash({})", bs58::encode(&self.0).into_string())
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
