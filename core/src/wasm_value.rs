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

/// Value that a Wasm function can accept or produce.
#[derive(Debug, Copy, Clone)]
pub enum WasmValue {
    /// A 32-bits integer. There is no fundamental difference between signed and unsigned
    /// integer, and the signed-ness should be determined depending on the context.
    I32(i32),
    /// A 32-bits integer. There is no fundamental difference between signed and unsigned
    /// integer, and the signed-ness should be determined depending on the context.
    I64(i64),
    /// A 32-bits floating point number.
    ///
    /// Contains the bits representation of the float.
    F32(u32),
    /// A 32-bits floating point number.
    ///
    /// Contains the bits representation of the float.
    F64(u64),
}

// TODO: what about U32/U64/etc.?
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ValueType {
    I32,
    I64,
    F32,
    F64,
}

impl WasmValue {
    /// Returns the type corresponding to this value.
    pub fn ty(&self) -> ValueType {
        match self {
            WasmValue::I32(_) => ValueType::I32,
            WasmValue::I64(_) => ValueType::I64,
            WasmValue::F32(_) => ValueType::F32,
            WasmValue::F64(_) => ValueType::F64,
        }
    }

    /// Unwraps [`WasmValue::I32`] into its value.
    pub fn into_i32(self) -> Option<i32> {
        if let WasmValue::I32(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Unwraps [`WasmValue::I64`] into its value.
    pub fn into_i64(self) -> Option<i64> {
        if let WasmValue::I64(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl From<wasmi::RuntimeValue> for WasmValue {
    fn from(val: wasmi::RuntimeValue) -> Self {
        match val {
            wasmi::RuntimeValue::I32(v) => WasmValue::I32(v),
            wasmi::RuntimeValue::I64(v) => WasmValue::I64(v),
            _ => unimplemented!(),
        }
    }
}

impl From<WasmValue> for wasmi::RuntimeValue {
    fn from(val: WasmValue) -> Self {
        match val {
            WasmValue::I32(v) => wasmi::RuntimeValue::I32(v),
            WasmValue::I64(v) => wasmi::RuntimeValue::I64(v),
            _ => unimplemented!(),
        }
    }
}

impl From<wasmi::ValueType> for ValueType {
    fn from(val: wasmi::ValueType) -> Self {
        match val {
            wasmi::ValueType::I32 => ValueType::I32,
            wasmi::ValueType::I64 => ValueType::I64,
            wasmi::ValueType::F32 => ValueType::F32,
            wasmi::ValueType::F64 => ValueType::F64,
        }
    }
}
