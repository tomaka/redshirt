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

/// Communication between a process and the interface handler.
///
/// A log message consists of one byte indicating the log level, followed with the log message
/// itself encoded in UTF-8.
///
/// Log levels:
///
/// - Error: 4
/// - Warn: 3
/// - Info: 2
/// - Debug: 1
/// - Trace: 0
///
use core::{convert::TryFrom, str};
use redshirt_syscalls_interface::{Decode, EncodedMessage, InterfaceHash};

// TODO: this has been randomly generated; instead should be a hash or something
pub const INTERFACE: InterfaceHash = InterfaceHash::from_raw_hash([
    0xa6, 0xbc, 0x8d, 0xc3, 0x43, 0xbd, 0xdd, 0x3b, 0x44, 0x2f, 0x06, 0x40, 0xa8, 0x40, 0xad, 0x4f,
    0x25, 0x57, 0x23, 0x91, 0x79, 0xc8, 0x16, 0x07, 0x6f, 0xab, 0xa9, 0xd6, 0x38, 0xca, 0x01, 0x8b,
]);

/// Log level of a message.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<Level> for u8 {
    fn from(level: Level) -> u8 {
        match level {
            Level::Error => 4,
            Level::Warn => 3,
            Level::Info => 2,
            Level::Debug => 1,
            Level::Trace => 0,
        }
    }
}

impl TryFrom<u8> for Level {
    type Error = (); // TODO:

    fn try_from(value: u8) -> Result<Self, ()> {
        Ok(match value {
            4 => Level::Error,
            3 => Level::Warn,
            2 => Level::Info,
            1 => Level::Debug,
            0 => Level::Trace,
            _ => return Err(()),
        })
    }
}

impl Decode for DecodedLogMessage {
    type Error = (); // TODO:

    fn decode(buffer: EncodedMessage) -> Result<Self, ()> {
        if buffer.0.is_empty() {
            return Err(());
        }
        let level = Level::try_from(buffer.0[0])?;
        let _ = str::from_utf8(&buffer.0[1..]).map_err(|_| ())?;
        Ok(DecodedLogMessage { level, buffer })
    }
}

/// Decoded version of a message on the log interface.
pub struct DecodedLogMessage {
    level: Level,
    buffer: EncodedMessage,
}

impl DecodedLogMessage {
    /// Returns the log level of the message.
    pub fn level(&self) -> Level {
        self.level
    }

    /// Returns the message itself.
    pub fn message(&self) -> &str {
        // We checked the validity when decoding.
        str::from_utf8(&self.buffer.0[1..]).unwrap()
    }
}
