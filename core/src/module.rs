// Copyright(c) 2019 Pierre Krieger

use sha2::Digest as _;
use std::fmt;

pub struct Module {
    inner: wasmi::Module,
    hash: ModuleHash,
}

/// Hash of a module.
#[derive(Clone, PartialEq, Eq)]
pub struct ModuleHash([u8; 32]);

impl Module {
    /// Parses a module from WASM bytes.
    pub fn from_bytes(buffer: impl AsRef<[u8]>) -> Self {
        let inner = wasmi::Module::from_buffer(buffer.as_ref()).unwrap();
        let hash = ModuleHash::from_bytes(buffer);

        Module {
            inner,
            hash,
        }
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
