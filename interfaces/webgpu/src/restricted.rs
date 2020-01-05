// Copyright (C) 2020  Pierre Krieger
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

use alloc::{string::String, vec::Vec};
use core::{convert::TryFrom, fmt};

/// Wrapper around `f32` that only allows finite values, and no infinite/NaN.
#[derive(Copy, Clone, PartialEq)]
pub struct RestrictedF32(f32);

impl TryFrom<f32> for RestrictedF32 {
    type Error = (); // TODO:

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        if !value.is_finite() {
            Ok(RestrictedF32(value))
        } else {
            Err(())
        }
    }
}

impl From<RestrictedF32> for f32 {
    fn from(val: RestrictedF32) -> f32 {
        val.0
    }
}

impl parity_scale_codec::Decode for RestrictedF32 {
    fn decode<I: parity_scale_codec::Input>(value: &mut I) -> Result<Self, parity_scale_codec::Error> {
        let bits = u32::decode(value)?;
        let float = f32::from_bits(bits);
        if !float.is_finite() {
            return Err(parity_scale_codec::Error::from("Decoded Infinite or NaN"))
        }
        Ok(RestrictedF32(float))
    }
}

impl parity_scale_codec::Encode for RestrictedF32 {
    fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
        // TODO: is that actually portable?
        self.0.to_bits().using_encoded(f)
    }
}

impl fmt::Debug for RestrictedF32 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

/// Wrapper around `f64` that only allows finite values, and no infinite/NaN.
#[derive(Copy, Clone, PartialEq)]
pub struct RestrictedF64(f64);

impl TryFrom<f64> for RestrictedF64 {
    type Error = (); // TODO:

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if !value.is_finite() {
            Ok(RestrictedF64(value))
        } else {
            Err(())
        }
    }
}

impl From<RestrictedF64> for f64 {
    fn from(val: RestrictedF64) -> f64 {
        val.0
    }
}

impl parity_scale_codec::Decode for RestrictedF64 {
    fn decode<I: parity_scale_codec::Input>(value: &mut I) -> Result<Self, parity_scale_codec::Error> {
        let bits = u64::decode(value)?;
        let float = f64::from_bits(bits);
        if !float.is_finite() {
            return Err(parity_scale_codec::Error::from("Decoded Infinite or NaN"))
        }
        Ok(RestrictedF64(float))
    }
}

impl parity_scale_codec::Encode for RestrictedF64 {
    fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
        // TODO: is that actually portable?
        self.0.to_bits().using_encoded(f)
    }
}

impl fmt::Debug for RestrictedF64 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}
