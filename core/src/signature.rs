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

use crate::ValueType;

use alloc::vec::Vec;
use smallvec::SmallVec;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Signature {
    params: SmallVec<[ValueType; 2]>,
    ret_ty: Option<ValueType>,
}

/// Easy way to generate a [`Signature`](crate::signature::Signature).
///
/// # Example
///
/// ```
/// let _sig: redshirt_core::signature::Signature = redshirt_core::sig!((I32, I64) -> I32);
/// ```
#[macro_export]
macro_rules! sig {
    (($($p:ident),*)) => {{
        let params = core::iter::empty();
        $(let params = params.chain(core::iter::once($crate::ValueType::$p));)*
        $crate::signature::Signature::new(params, None)
    }};
    (($($p:ident),*) -> $ret:ident) => {{
        let params = core::iter::empty();
        $(let params = params.chain(core::iter::once($crate::ValueType::$p));)*
        $crate::signature::Signature::new(params, Some($crate::ValueType::$ret))
    }};
}

impl Signature {
    pub fn new(
        params: impl Iterator<Item = ValueType>,
        ret_ty: impl Into<Option<ValueType>>,
    ) -> Signature {
        Signature {
            params: params.collect(),
            ret_ty: ret_ty.into(),
        }
    }

    /// Returns a list of all the types of the parameters.
    pub fn parameters(&self) -> impl ExactSizeIterator<Item = &ValueType> {
        self.params.iter()
    }

    /// Returns the type of the return type of the function. `None` means "void".
    pub fn return_type(&self) -> &Option<ValueType> {
        &self.ret_ty
    }

    pub(crate) fn matches_wasmi(&self, sig: &wasmi::Signature) -> bool {
        wasmi::Signature::from(self) == *sig
    }
}

impl<'a> From<&'a Signature> for wasmi::Signature {
    fn from(sig: &'a Signature) -> wasmi::Signature {
        wasmi::Signature::new(
            sig.params
                .iter()
                .cloned()
                .map(wasmi::ValueType::from)
                .collect::<Vec<_>>(),
            sig.ret_ty.map(wasmi::ValueType::from),
        )
    }
}

impl From<Signature> for wasmi::Signature {
    fn from(sig: Signature) -> wasmi::Signature {
        wasmi::Signature::from(&sig)
    }
}

impl From<ValueType> for wasmi::ValueType {
    fn from(ty: ValueType) -> wasmi::ValueType {
        match ty {
            ValueType::I32 => wasmi::ValueType::I32,
            ValueType::I64 => wasmi::ValueType::I64,
            ValueType::F32 => wasmi::ValueType::F32,
            ValueType::F64 => wasmi::ValueType::F64,
        }
    }
}
