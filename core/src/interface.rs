// Copyright(c) 2019 Pierre Krieger

use sha2::{Digest as _, digest::FixedOutput as _};
use std::fmt;

/// Definition of an interface.
pub struct Interface {
    name: String,
    functions: Vec<Function>,
    hash: InterfaceHash,
}

/// Prototype of an interface being built.
pub struct InterfaceBuilder {
    name: String,
    functions: Vec<Function>,
}

/// Hash of an interface definition.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct InterfaceHash([u8; 32]);

struct Function {
    name: String,
    signature: wasmi::Signature,
}

impl Interface {
    /// Starts building an [`Interface`] with an [`InterfaceBuilder`].
    pub fn new() -> InterfaceBuilder {
        InterfaceBuilder {
            name: String::new(),
            functions: Vec::new(),
        }
    }

    /// Returns the hash of the interface.
    pub fn hash(&self) -> &InterfaceHash {
        &self.hash
    }
}

impl InterfaceBuilder {
    /// Changes the name of the prototype interface.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Adds a function to the prototype interface.
    // TODO: don't expose wasmi types in the API
    pub fn with_function(mut self, name: impl Into<String>, signature: impl Into<wasmi::Signature>) -> Self {
        self.functions.push(Function {
            name: name.into(),
            signature: signature.into(),
        });
        self
    }

    /// Turns the builder into an actual interface.
    pub fn build(mut self) -> Interface {
        self.functions.shrink_to_fit();

        // Let's build the hash of our interface.
        let mut hash_state = sha2::Sha256::default();
        hash_state.input(self.name.as_bytes());
        // TODO: hash the function definitions as well
        // TODO: need some delimiter between elements of the hash, otherwise people can craft
        //       collisions

        Interface {
            name: self.name,
            functions: self.functions,
            hash: InterfaceHash(hash_state.fixed_result().into()),
        }
    }
}

impl From<[u8; 32]> for InterfaceHash {
    fn from(hash: [u8; 32]) -> InterfaceHash {
        InterfaceHash(hash)
    }
}

impl fmt::Display for InterfaceHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&bs58::encode(&self.0).into_string(), f)
    }
}

impl fmt::Debug for InterfaceHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "InterfaceHash({})", bs58::encode(&self.0).into_string())
    }
}
