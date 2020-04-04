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
//!
//! # Panic-free code
//!
//! The code within this module is designed to be as panic-free as possible. In other words, you
//! can assume that a [`KLogger`] will be capable of printing a panic message without itself
//! triggering a nested panic.
//!
//! In particular, none of the code within this module does any heap allocation. Note that APIs
//! that use a [`KLogger`] typically wrap it within an `Arc`. In order to account for possible
//! allocation errors during the allocation of this `Arc`, one is encouraged to create a default
//! fallback [`KLogger`] (using the const [`KLogger::new`] method) in order to print potential
//! panic messages before the actual [`KLogger`] is properly set up.

pub use logger::KLogger;

mod logger;
mod native;
mod video;
