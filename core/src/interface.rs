// Copyright(c) 2019 Pierre Krieger

use sha2::Digest as _;
use std::fmt;

/// Definition of an interface.
pub struct Interface {
    name: String,
    functions: Vec<Function>,
    hash: InterfaceHash,
}

/// Hash of an interface definition.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct InterfaceHash([u8; 32]);

struct Function {
    name: String,
    signature: wasmi::Signature,
}

impl Interface {
    /// Returns the hash of the interface.
    pub fn hash(&self) -> &InterfaceHash {
        &self.hash
    }
}

impl fmt::Debug for InterfaceHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "InterfaceHash({})", bs58::encode(&self.0).into_string())
    }
}
