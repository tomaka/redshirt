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

use alloc::{borrow::Cow, vec::Vec};
use core::fmt;

/// Message already encoded.
///
/// The [`Encode`] and [`Decode`] trait implementations are no-op.
#[derive(Clone, PartialEq, Eq)]
pub struct EncodedMessage(pub Vec<u8>);

/// Objects that represent messages that can be serialized in order to be sent on an interface.
pub trait Encode {
    /// Turn the object into bytes ready to be transmitted.
    fn encode(self) -> EncodedMessage;
}

/// Objects that represent messages that can be unserialized.
pub trait Decode {
    type Error: fmt::Debug;

    /// Decode the raw data passed as parameter.
    fn decode(buffer: EncodedMessage) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

impl EncodedMessage {
    pub fn decode<T: Decode>(self) -> Result<T, T::Error> {
        T::decode(self)
    }
}

impl Encode for EncodedMessage {
    fn encode(self) -> EncodedMessage {
        self
    }
}

impl<T> Encode for T
where
    T: parity_scale_codec::Encode,
{
    fn encode(self) -> EncodedMessage {
        EncodedMessage(parity_scale_codec::Encode::encode(&self))
    }
}

impl Decode for EncodedMessage {
    type Error = core::convert::Infallible; // TODO: `!`

    fn decode(buffer: EncodedMessage) -> Result<Self, Self::Error> {
        Ok(buffer)
    }
}

impl<T> Decode for T
where
    T: parity_scale_codec::DecodeAll,
{
    type Error = ();

    fn decode(buffer: EncodedMessage) -> Result<Self, Self::Error> {
        parity_scale_codec::DecodeAll::decode_all(&buffer.0).map_err(|_| ())
    }
}

impl fmt::Debug for EncodedMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}
