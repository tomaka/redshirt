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

//! Stdout.

#![deny(intra_doc_link_resolution_failure)]
#![no_std]

extern crate alloc;

use alloc::{format, string::String};

pub mod ffi;

pub use ffi::Level;

/// Appends a string to the logs of the program.
///
/// # About `\r` vs `\n`
///
/// In order to follow the Unix world, the character `\n` (LF, 0xA) means "new line". The
/// character `\r` (CR, 0xD) is ignored.
pub fn log(level: Level, msg: String) {
    unsafe {
        let msg = ffi::LogMessage::Message(level, msg);
        redshirt_syscalls_interface::emit_message_without_response(&ffi::INTERFACE, &msg).unwrap();
    }
}

pub struct EnvLogger;

pub fn try_init() -> Result<(), log::SetLoggerError> {
    static LOGGER: EnvLogger = EnvLogger;
    log::set_logger(&LOGGER)
}

pub fn init() {
    try_init().unwrap();
}

impl log::Log for EnvLogger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let level = match record.level() {
            log::Level::Error => Level::Error,
            log::Level::Warn => Level::Warn,
            log::Level::Info => Level::Info,
            log::Level::Debug => Level::Debug,
            log::Level::Trace => Level::Trace,
        };

        // TODO: ideally we wouldn't allocate any memory in order to print out
        log(level, format!("{}:{} -- {}", record.level(), record.target(), record.args()));
    }

    fn flush(&self) {
    }
}
