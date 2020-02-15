// Copyright (C) 2019-2020  Pierre Krieger
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

//! Logging.
//!
//! This interface allows a program to send out messages for the purpose of being logged.
//!
//! How these logs are handled is at the discretion of the rest of the system, but the intent is
//! for them to be shown to a human being if desired.

#![deny(intra_doc_link_resolution_failure)]
#![no_std]

extern crate alloc;

use alloc::format;

pub mod ffi;

pub use ffi::Level;

/// Appends a single string to the logs of the program.
///
/// This function always adds a single entry to the logs. An entry can made up of multiple lines
/// (separated with `\n`), but the lines are notably *not* split into multiple entries.
///
/// # About `\r` vs `\n`
///
/// In order to follow the Unix world, the character `\n` (LF, 0xA) means "new line". The
/// character `\r` (CR, 0xD) is ignored.
///
pub fn log(level: Level, msg: &str) {
    unsafe {
        let level: [u8; 1] = [u8::from(level)];
        redshirt_syscalls::MessageBuilder::new()
            .add_data_raw(&level[..])
            .add_data_raw(msg.as_bytes())
            .emit_without_response(&ffi::INTERFACE)
            .unwrap();
    }
}

/// Attempts to initializes the global logger.
///
/// # Panic
///
/// This function will panic if it is called more than once, or if another library has already
/// initialized a global logger.
pub fn try_init() -> Result<(), log::SetLoggerError> {
    static LOGGER: GlobalLogger = GlobalLogger;
    let res = log::set_logger(&LOGGER);
    if res.is_ok() {
        log::set_max_level(log::LevelFilter::Trace);
    }
    res
}

/// Initializes the global logger.
///
/// # Panic
///
/// This function will panic if it is called more than once, or if another library has already
/// initialized a global logger.
pub fn init() {
    try_init().unwrap();
}

/// The logger.
///
/// Implements the [`Log`](log::Log) trait.
pub struct GlobalLogger;

impl log::Log for GlobalLogger {
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

        let message = format!("{} -- {}", record.target(), record.args());
        log(level, &message)
    }

    fn flush(&self) {}
}
