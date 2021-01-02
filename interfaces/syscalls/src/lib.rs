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

//! Bindings for interfacing with the environment of the "kernel".
//!
//! # Messages and responses
//!
//! The environment available to `redshirt` programs consists in a collection of **interfaces**.
//! An interface is referred to by a 32-bytes hash.
//!
//! Programs can emit messages by passing a target interface (a 32 bytes array), and a buffer
//! containing the body of the message. The way the body of the message must be interpreted is
//! entirely dependant on the interface it is sent on. Emitting a message always succeeds if the
//! interface is available to the program, even if the body is malformed.
//!
//! When emitting a message, the sender must indicate whether or not it expects a response. If the
//! interface handler sends back a response when none is expected, the response is discarded. If
//! the interface handler doesn't send back a response when one is expected, then you effectively
//! have a memory leak.
//!
//! A response can also be cancelled by the sender, in which case it is as if it had decided to not
//! expect any response.
//!
//! The two primary and recommended ways to emit a message are the
//! [`emit_message_without_response`] and [`emit_message_with_response`] functions.
//!
//! # Interface handling
//!
//! A program can register itself as an interface handler. This can be done by sending a message
//! on a special-cased interface, called the "interface" interface. This is out of scoope of this
//! crate. See the `redshirt_interface_interface` crate instead.
//!
//! # About threads
//!
//! Multithreading in WASM isn't specified yet, and Rust doesn't allow multithreaded WASM code.
//! In particular, multithreaded WASM code in LLVM is undefined behaviour.
//!
//! With that in mind, this makes writing an implementation of `Future` challenging. When the
//! `Future` returns `Poll::Pending`, the `Waker` has to be stored somewhere and invoked. Since
//! there is no possibility of having multiple threads, the only moment when the `Waker` can be
//! invoked is when we explicitly call a function whose role is to do that. The only reasonable
//! choice for such function is the [`block_on()`] function, or similar functions.
//!
//! For the same reason, it is also challenging to write an implementation of [`block_on()`].
//! Putting the current thread to sleep is not enough, because the lack of background threads
//! makes it impossible for the `Waker` to be invoked. An implementation of [`block_on()`] **must**
//! somehow perform actions that will drive to completion the `Future` it is blocking upon,
//! otherwise nothing will ever happen.
//!
//! Consequently, it has been decided that the implementations of `Future` that this module
//! provide interact, through a global variable, with the behaviour of [`block_on()`]. More
//! precisely, before a `Future` returns `Poll::Pending`, it stores its `Waker` in a global
//! variable alongside with the ID of the message whose response we are waiting for, and the
//! [`block_on()`] function reads and processes that global variable.
//!
//! It is not possible to build a `Future` that is not built on top of one of the `Future`
//! provided by this crate, and every single use-cases of `Future`s that we could think of
//! can and must be built on top of them. Similarly, it is not possible to build an implementation
//! of [`block_on()`] without having access to the internals of these `Future`s. Tying these
//! `Future`s to the implementation of [`block_on()`] is therefore the logical thing to do.
//!

#![no_std]

extern crate alloc;

pub use block_on::block_on;
pub use emit::{
    cancel_message, emit_message_with_response, emit_message_without_response, MessageBuilder,
};
pub use ffi::DecodedNotificationRef;
pub use response::{message_response, message_response_sync_raw, MessageResponseFuture};
pub use traits::{Decode, Encode, EncodedMessage, EncodedMessageRef};

use core::{cmp::PartialEq, convert::TryFrom, fmt, num::NonZeroU64};

mod block_on;
mod emit;
mod response;
mod traits;

pub mod ffi;

/// Identifier of a running process within a core.
// TODO: move to a Pid module?
// TODO: should be NonZeroU64?
#[derive(
    Copy, Clone, PartialEq, Eq, Hash, parity_scale_codec::Encode, parity_scale_codec::Decode,
)]
pub struct Pid(u64);

impl From<NonZeroU64> for Pid {
    fn from(id: NonZeroU64) -> Pid {
        Pid(id.get())
    }
}

impl From<u64> for Pid {
    fn from(id: u64) -> Pid {
        Pid(id)
    }
}

impl From<Pid> for u64 {
    fn from(pid: Pid) -> u64 {
        pid.0
    }
}

impl fmt::Debug for Pid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "#{:020}", self.0)
    }
}

/// Identifier of a running thread within a core.
// TODO: move to a separate module?
// TODO: should be NonZeroU64?
#[derive(
    Copy, Clone, PartialEq, Eq, Hash, parity_scale_codec::Encode, parity_scale_codec::Decode,
)]
pub struct ThreadId(u64);

impl From<NonZeroU64> for ThreadId {
    fn from(id: NonZeroU64) -> ThreadId {
        ThreadId(id.get())
    }
}

impl From<u64> for ThreadId {
    fn from(id: u64) -> ThreadId {
        ThreadId(id)
    }
}

impl From<ThreadId> for u64 {
    fn from(tid: ThreadId) -> u64 {
        tid.0
    }
}

impl fmt::Debug for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "#{:020}", self.0)
    }
}

/// Identifier of a message to answer.
// TODO: move to a MessageId module?
#[derive(
    Copy, Clone, PartialEq, Eq, Hash, parity_scale_codec::Encode, parity_scale_codec::Decode,
)]
pub struct MessageId(NonZeroU64);

impl MessageId {
    /// Turns a raw integer into a [`MessageId`] without checking its validity.
    ///
    /// # Safety
    ///
    /// `id` must not be equal to 0.
    pub unsafe fn from_u64_unchecked(id: u64) -> Self {
        MessageId(NonZeroU64::new_unchecked(id))
    }
}

impl TryFrom<u64> for MessageId {
    type Error = InvalidMessageIdErr;

    fn try_from(id: u64) -> Result<Self, Self::Error> {
        match id {
            0 => Err(InvalidMessageIdErr),
            n => Ok(MessageId(NonZeroU64::new(n).unwrap())),
        }
    }
}

impl TryFrom<NonZeroU64> for MessageId {
    type Error = InvalidMessageIdErr;

    fn try_from(id: NonZeroU64) -> Result<Self, Self::Error> {
        Ok(MessageId(id))
    }
}

impl From<MessageId> for NonZeroU64 {
    fn from(mid: MessageId) -> NonZeroU64 {
        mid.0
    }
}

impl From<MessageId> for u64 {
    fn from(mid: MessageId) -> u64 {
        mid.0.get()
    }
}

impl fmt::Debug for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "#{:020}", self.0)
    }
}

/// Error when trying to build a [`MessageId`] from a raw id.
#[derive(Debug)]
pub struct InvalidMessageIdErr;

impl fmt::Display for InvalidMessageIdErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Invalid message ID")
    }
}

/// Hash of a module.
#[derive(Clone, parity_scale_codec::Encode, parity_scale_codec::Decode, PartialEq, Eq, Hash)]
pub struct InterfaceHash([u8; 32]);

impl InterfaceHash {
    /// Builds the [`InterfaceHash`] given the raw bytes.
    pub const fn from_raw_hash(hash: [u8; 32]) -> Self {
        InterfaceHash(hash)
    }
}

impl AsRef<[u8]> for InterfaceHash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<InterfaceHash> for [u8; 32] {
    fn from(interface: InterfaceHash) -> [u8; 32] {
        interface.0
    }
}

impl From<[u8; 32]> for InterfaceHash {
    fn from(hash: [u8; 32]) -> InterfaceHash {
        InterfaceHash(hash)
    }
}

impl PartialEq<[u8; 32]> for InterfaceHash {
    fn eq(&self, other: &[u8; 32]) -> bool {
        self.0 == *other
    }
}

impl PartialEq<InterfaceHash> for [u8; 32] {
    fn eq(&self, other: &InterfaceHash) -> bool {
        *self == other.0
    }
}

impl fmt::Debug for InterfaceHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "InterfaceHash(0x")?;
        for byte in &self.0 {
            write!(f, "{:02x}", *byte)?
        }
        write!(f, ")")?;
        Ok(())
    }
}
