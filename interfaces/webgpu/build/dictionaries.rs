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

use case::CaseExt as _;
use std::{borrow::Cow, env, fs, io::{self, Write}, path::Path};
use webidl::ast;

pub fn gen_types(out: &mut impl Write, idl: &ast::AST) -> Result<(), io::Error> {
    for definition in idl {
        match definition {
            ast::Definition::Callback(_) => unimplemented!(),
            ast::Definition::Dictionary(ast::Dictionary::NonPartial(dictionary)) => {
                // We don't support any attribute.
                // TODO: assert!(dictionary.extended_attributes.is_empty());
                writeln!(out, "#[derive(Debug, parity_scale_codec::Encode, parity_scale_codec::Decode)]")?;
                writeln!(out, "pub struct {} {{", dictionary.name)?;
                if let Some(inherit) = dictionary.inherits.as_ref() {
                    writeln!(out, "    pub r#parent: {},", inherit)?;
                }
                for member in dictionary.members.iter() {
                    // We don't support any attribute.
                    assert!(member.extended_attributes.is_empty());
                    writeln!(out, "    pub r#{}: {},", member.name.to_snake(), ty_to_rust(&member.type_))?;
                }
                writeln!(out, "}}")?;
            },
            ast::Definition::Dictionary(ast::Dictionary::Partial(_)) => unimplemented!(),
            ast::Definition::Enum(en) => {
                // We don't support any attribute.
                assert!(en.extended_attributes.is_empty());
                writeln!(out, "#[derive(Debug, parity_scale_codec::Encode, parity_scale_codec::Decode)]")?;
                writeln!(out, "pub enum {} {{", en.name)?;
                for variant in en.variants.iter() {
                    let mut variant = variant.replace('-', "_").to_camel();
                    if variant.chars().next().unwrap().is_digit(10) {
                        variant = format!("V{}", variant);
                    }
                    writeln!(out, "    {},", variant)?;
                }
                writeln!(out, "}}")?;
            },
            ast::Definition::Implements(_) => unimplemented!(),
            ast::Definition::Includes(_) => {},
            ast::Definition::Interface(ast::Interface::Callback(_)) => unimplemented!(),
            ast::Definition::Interface(ast::Interface::Partial(interface)) => {}
            ast::Definition::Interface(ast::Interface::NonPartial(interface)) => {},
            ast::Definition::Mixin(_) => {},
            ast::Definition::Namespace(_) => unimplemented!(),
            ast::Definition::Typedef(typedef) => {
                // We don't support any attribute.
                assert!(typedef.extended_attributes.is_empty());
                writeln!(out, "pub type {} = {};", typedef.name, ty_to_rust(&typedef.type_))?;
            },
        }
    }

    Ok(())
}

fn ty_to_rust(ty: &ast::Type) -> Cow<'static, str> {
    // We don't support any attribute.
    assert!(ty.extended_attributes.is_empty());

    let outcome = match &ty.kind {
        ast::TypeKind::ArrayBuffer => From::from("ArrayBuffer"), // TODO: need to figure this out
        ast::TypeKind::Boolean => From::from("bool"),
        ast::TypeKind::Byte => From::from("i8"), // Note: A `Byte` is signed. This is not a mistake.
        ast::TypeKind::DOMString => From::from("String"),
        ast::TypeKind::Identifier(id) => From::from(id.clone()),
        ast::TypeKind::Octet => From::from("u8"),
        ast::TypeKind::Promise(output_ty) => {
            let out_ty = match &**output_ty {
                ast::ReturnType::Void => From::from("void"),
                ast::ReturnType::NonVoid(ty) => ty_to_rust(ty),
            };
            From::from(format!("Pin<Box<dyn Future<Output = {}>>>", out_ty))
        },
        ast::TypeKind::RestrictedFloat => From::from("RestrictedF32"),
        ast::TypeKind::RestrictedDouble => From::from("RestrictedF64"),
        ast::TypeKind::Sequence(elem_ty) => From::from(format!("Vec<{}>", ty_to_rust(elem_ty))),
        ast::TypeKind::SignedLong => From::from("i32"),
        ast::TypeKind::SignedLongLong => From::from("i64"),
        ast::TypeKind::SignedShort => From::from("i16"),
        ast::TypeKind::Uint32Array => From::from("Vec<u32>"),
        ast::TypeKind::Union(ty_list) => ty_to_rust(&ty_list[0]),       // FIXME: hack
        ast::TypeKind::UnrestrictedFloat => From::from("f32"),
        ast::TypeKind::UnsignedLong => From::from("u32"),
        ast::TypeKind::UnrestrictedDouble => From::from("f64"),
        ast::TypeKind::UnsignedLongLong => From::from("u64"),
        ast::TypeKind::UnsignedShort => From::from("u16"),
        t => unimplemented!("{:?}", t),
    };

    // TODO: is that correct?
    if ty.nullable {
        From::from(format!("Option<{}>", outcome))
    } else {
        outcome
    }
}
