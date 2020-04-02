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

//! Kernel logs handling.
//!
//! This module handles the way the kernel prints logs. It provides the [`KLogger`] structure
//! that needs to be configured with a certain logging output method, and is then capable of
//! outputting logs.
//!
//! After you have initialized a [`KLogger`], you should also call
//! [`crate::arch::PlatformSpecific::set_panic_logger`] in order for the panic handler to use it
//! to print panic log messages.

pub use logger::{KLogger, PanicPrinter};

mod logger;
