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

use std::{borrow::Cow, env, fs, io::Write as _, path::Path};
use webidl::ast;

pub struct ConvertState {
    pub unions: HashMap<>,
}

impl ConvertState {
    let mut out = {
        let dest_path = Path::new(&env::var("OUT_DIR").unwrap()).join("webgpu.rs");
        fs::File::create(&dest_path).unwrap()
    };

    for definition in &parse::gen_parsed_idl() {
        match definition {
            ast::Definition::Callback(_) => unimplemented!(),
            ast::Definition::Dictionary(ast::Dictionary::NonPartial(dictionary)) => {
                // We don't support any attribute.
                assert!(dictionary.extended_attributes.is_empty());
                assert!(dictionary.inherits.is_none()); // TODO: not implemented
                writeln!(out, "pub struct {} {{", dictionary.name).unwrap();
                for member in dictionary.members.iter() {
                    // We don't support any attribute.
                    assert!(member.extended_attributes.is_empty());
                    writeln!(out, "pub {}: {}", member.name, ty_to_rust(&member.type_)).unwrap();
                }
                writeln!(out, "}}").unwrap();
            },
            ast::Definition::Dictionary(ast::Dictionary::Partial(_)) => unimplemented!(),
            ast::Definition::Enum(_) => unimplemented!(),
            ast::Definition::Implements(_) => unimplemented!(),
            ast::Definition::Includes(_) => unimplemented!(),
            ast::Definition::Interface(_) => unimplemented!(),
            ast::Definition::Mixin(_) => unimplemented!(),
            ast::Definition::Namespace(_) => unimplemented!(),
            ast::Definition::Typedef(typedef) => {
                // We don't support any attribute.
                assert!(typedef.extended_attributes.is_empty());
                writeln!(out, "pub type {} = {};", typedef.name, ty_to_rust(&typedef.type_)).unwrap();
            },
        }
    }

    fn ty_to_rust(&mut self, ty: &ast::Type) -> Cow<'static, str> {
        // We don't support any attribute.
        assert!(ty.extended_attributes.is_empty());
        assert!(!ty.nullable);      // TODO: what is this?

        match &ty.kind {
            ast::TypeKind::Boolean => From::from("bool"),
            ast::TypeKind::Byte => From::from("i8"), // Note: A `Byte` is signed. This is not a mistake.
            ast::TypeKind::Identifier(id) => From::from(id.clone()),
            ast::TypeKind::Octet => From::from("u8"),
            ast::TypeKind::RestrictedFloat => From::from("f32"),   // FIXME: "restricted" means can't be infinite
            ast::TypeKind::RestrictedDouble => From::from("f64"),   // FIXME: "restricted" means can't be infinite
            ast::TypeKind::Sequence(elem_ty) => From::from(format!("Vec<{}>", ty_to_rust(elem_ty))),
            ast::TypeKind::SignedLong => From::from("i32"),
            ast::TypeKind::SignedLongLong => From::from("i64"),
            ast::TypeKind::SignedShort => From::from("i16"),
            ast::TypeKind::Union(ty_list) => ty_to_rust(&ty_list[0]),       // FIXME: hack
            ast::TypeKind::UnrestrictedFloat => From::from("f32"),
            ast::TypeKind::UnsignedLong => From::from("u32"),
            ast::TypeKind::UnrestrictedDouble => From::from("f64"),
            ast::TypeKind::UnsignedLongLong => From::from("u64"),
            ast::TypeKind::UnsignedShort => From::from("u16"),
            t => unimplemented!("{:?}", t),
        }
    }
}
