// Copyright(c) 2019 Pierre Krieger

use crate::signature::Signature;
use sha2::{digest::FixedOutput as _, Digest as _};
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

/// Identifier of an interface. Can be either a hash or a string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InterfaceId {
    Hash(InterfaceHash),
    Bytes(String),
}

/// Hash of an interface definition.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct InterfaceHash([u8; 32]);

struct Function {
    name: String,
    signature: Signature,
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
    pub fn with_function(
        mut self,
        name: impl Into<String>,
        signature: impl Into<Signature>,
    ) -> Self {
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

impl From<[u8; 32]> for InterfaceId {
    fn from(hash: [u8; 32]) -> InterfaceId {
        InterfaceId::Hash(hash.into())
    }
}

impl From<String> for InterfaceId {
    fn from(name: String) -> InterfaceId {
        InterfaceId::Bytes(name)
    }
}

impl<'a> From<&'a str> for InterfaceId {
    fn from(name: &'a str) -> InterfaceId {
        InterfaceId::Bytes(name.to_owned())
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
