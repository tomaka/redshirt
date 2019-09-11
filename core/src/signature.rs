// Copyright(c) 2019 Pierre Krieger

use smallvec::SmallVec;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Signature {
    params: SmallVec<[ValueType; 2]>,
    ret_ty: Option<ValueType>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ValueType {
    Pointer,
    I32,
    I64,
    F32,
    F64,
}

impl Signature {
    pub fn new(params: impl Iterator<Item = ValueType>, ret_ty: impl Into<Option<ValueType>>) -> Signature {
        Signature {
            params: params.collect(),
            ret_ty: ret_ty.into(),
        }
    }

    pub(crate) fn matches_wasmi(&self, sig: &wasmi::Signature) -> bool {
        wasmi::Signature::from(self) == *sig
    }
}

impl<'a> From<&'a Signature> for wasmi::Signature {
    fn from(sig: &'a Signature) -> wasmi::Signature {
        wasmi::Signature::new(
            sig.params.iter().cloned().map(wasmi::ValueType::from).collect::<Vec<_>>(), 
            sig.ret_ty.map(wasmi::ValueType::from)
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
            ValueType::Pointer => wasmi::ValueType::I32,
            ValueType::I32 => wasmi::ValueType::I32,
            ValueType::I64 => wasmi::ValueType::I64,
            ValueType::F32 => wasmi::ValueType::F32,
            ValueType::F64 => wasmi::ValueType::F64,
        }
    }
}
