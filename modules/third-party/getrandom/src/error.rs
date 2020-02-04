// Copyright 2018 Developers of the Rand project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
use core::fmt;
use core::num::NonZeroU32;

/// A small and `no_std` compatible error type.
///
/// The [`Error::raw_os_error()`] will indicate if the error is from the OS, and
/// if so, which error code the OS gave the application. If such an error is
/// encountered, please consult with your system documentation.
///
/// Internally this type is a NonZeroU32, with certain values reserved for
/// certain purposes, see [`Error::INTERNAL_START`] and [`Error::CUSTOM_START`].
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Error(NonZeroU32);

impl Error {
    /// Codes below this point represent OS Errors (i.e. positive i32 values).
    /// Codes at or above this point, but below [`Error::CUSTOM_START`] are
    /// reserved for use by the `rand` and `getrandom` crates.
    pub const INTERNAL_START: u32 = 1 << 31;

    /// Codes at or above this point can be used by users to define their own
    /// custom errors.
    pub const CUSTOM_START: u32 = (1 << 31) + (1 << 30);

    /// Extract the raw OS error code (if this error came from the OS)
    ///
    /// This method is identical to `std::io::Error::raw_os_error()`, except
    /// that it works in `no_std` contexts. If this method returns `None`, the
    /// error value can still be formatted via the `Display` implementation.
    #[inline]
    pub fn raw_os_error(self) -> Option<i32> {
        if self.0.get() < Self::INTERNAL_START {
            Some(self.0.get() as i32)
        } else {
            None
        }
    }

    /// Extract the bare error code.
    ///
    /// This code can either come from the underlying OS, or be a custom error.
    /// Use [`Error::raw_os_error()`] to disambiguate.
    #[inline]
    pub fn code(self) -> NonZeroU32 {
        self.0
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut dbg = f.debug_struct("Error");
        dbg.finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unknown Error: {}", self.0.get())
    }
}

impl From<NonZeroU32> for Error {
    fn from(code: NonZeroU32) -> Self {
        Self(code)
    }
}
