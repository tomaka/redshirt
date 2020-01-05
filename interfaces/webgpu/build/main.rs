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
use std::{borrow::Cow, env, fs, io::Write as _, path::Path};
use webidl::ast;

mod parse;

fn main() {
    let mut out_lib = {
        let dest_path = Path::new(&env::var("OUT_DIR").unwrap()).join("webgpu.rs");
        fs::File::create(&dest_path).unwrap()
    };

    let mut out_ffi = {
        let dest_path = Path::new(&env::var("OUT_DIR").unwrap()).join("ffi.rs");
        fs::File::create(&dest_path).unwrap()
    };

    let idl = parse::gen_parsed_idl();

    writeln!(out_ffi, "#[derive(Debug, Encode, Decode)]").unwrap();
    writeln!(out_ffi, "pub enum WebGPUMessage {{").unwrap();
    for definition in &idl {
        match definition {
            ast::Definition::Interface(ast::Interface::Partial(_)) => {} // FIXME: unimplemented!()
            ast::Definition::Interface(ast::Interface::NonPartial(interface)) => {
                for member in interface.members.iter() {
                    match member {
                        ast::InterfaceMember::Iterable(_) => unimplemented!(),
                        ast::InterfaceMember::Maplike(_) => unimplemented!(),
                        ast::InterfaceMember::Operation(ast::Operation::Regular(op)) => {
                            assert!(op.extended_attributes.is_empty());
                            if let Some(name) = op.name.as_ref() {
                                // FIXME: parameters
                                if let ast::ReturnType::NonVoid(ty) = &op.return_type {
                                    writeln!(out_ffi, "    // Response: {}", ty_to_rust(ty)).unwrap();
                                }
                                writeln!(out_ffi, "    {}{},", interface.name, name.to_camel()).unwrap();
                            } else {
                                // TODO: what is that???
                            }
                        },
                        ast::InterfaceMember::Operation(ast::Operation::Special(_)) => unimplemented!(),
                        ast::InterfaceMember::Operation(ast::Operation::Static(_)) => unimplemented!(),
                        ast::InterfaceMember::Operation(ast::Operation::Stringifier(_)) => unimplemented!(),
                        ast::InterfaceMember::Setlike(_) => unimplemented!(),
                        _ => {}     // FIXME:
                    }
                }
            },
            _ => {}
        }
    }
    writeln!(out_ffi, "}}").unwrap();

    for definition in &idl {
        match definition {
            ast::Definition::Callback(_) => unimplemented!(),
            ast::Definition::Dictionary(ast::Dictionary::NonPartial(dictionary)) => {
                // We don't support any attribute.
                // TODO: assert!(dictionary.extended_attributes.is_empty());
                // TODO: assert!(dictionary.inherits.is_none()); // TODO: not implemented
                writeln!(out_lib, "pub struct {} {{", dictionary.name).unwrap();
                for member in dictionary.members.iter() {
                    // We don't support any attribute.
                    assert!(member.extended_attributes.is_empty());
                    writeln!(out_lib, "    pub r#{}: {},", member.name.to_snake(), ty_to_rust(&member.type_)).unwrap();
                }
                writeln!(out_lib, "}}").unwrap();
            },
            ast::Definition::Dictionary(ast::Dictionary::Partial(_)) => unimplemented!(),
            ast::Definition::Enum(en) => {
                // We don't support any attribute.
                assert!(en.extended_attributes.is_empty());
                writeln!(out_lib, "pub enum {} {{", en.name).unwrap();
                for variant in en.variants.iter() {
                    let mut variant = variant.replace('-', "_").to_camel();
                    if variant.chars().next().unwrap().is_digit(10) {
                        variant = format!("V{}", variant);
                    }
                    writeln!(out_lib, "    {},", variant).unwrap();
                }
                writeln!(out_lib, "}}").unwrap();
            },
            ast::Definition::Implements(_) => unimplemented!(),
            ast::Definition::Includes(_) => {},
            ast::Definition::Interface(ast::Interface::Callback(_)) => unimplemented!(),
            ast::Definition::Interface(ast::Interface::Partial(interface)) => {} // FIXME: unimplemented!()
            ast::Definition::Interface(ast::Interface::NonPartial(interface)) => { // FIXME: unimplemented!()
                writeln!(out_lib, "pub struct {} {{", interface.name).unwrap();
                /*for member in interface.members.iter() {
                    // We don't support any attribute.
                    assert!(member.extended_attributes.is_empty());
                    writeln!(out_lib, "    pub r#{}: {},", member.name, ty_to_rust(&member.type_)).unwrap();
                }*/
                writeln!(out_lib, "}}").unwrap();
            },
            ast::Definition::Mixin(_) => {}, // FIXME: unimplemented!()
            ast::Definition::Namespace(_) => unimplemented!(),
            ast::Definition::Typedef(typedef) => {
                // We don't support any attribute.
                assert!(typedef.extended_attributes.is_empty());
                writeln!(out_lib, "pub type {} = {};", typedef.name, ty_to_rust(&typedef.type_)).unwrap();
            },
        }
    }
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
        ast::TypeKind::RestrictedFloat => From::from("f32"),   // FIXME: "restricted" means can't be infinite
        ast::TypeKind::RestrictedDouble => From::from("f64"),   // FIXME: "restricted" means can't be infinite
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
