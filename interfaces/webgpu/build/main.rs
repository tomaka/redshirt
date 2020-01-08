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

mod dictionaries;
mod ffi_bindings;
mod parse;

fn main() {
    let mut out_main = {
        let dest_path = Path::new(&env::var("OUT_DIR").unwrap()).join("webgpu.rs");
        fs::File::create(&dest_path).unwrap()
    };

    let mut out_ffi = {
        let dest_path = Path::new(&env::var("OUT_DIR").unwrap()).join("ffi.rs");
        fs::File::create(&dest_path).unwrap()
    };

    let idl = parse::gen_parsed_idl();

    gen_main(&mut out_main, &idl).unwrap();
    ffi_bindings::gen_ffi(&mut out_ffi, &idl).unwrap();
}

fn gen_main(out: &mut impl Write, idl: &ast::AST) -> Result<(), io::Error> {
    crate::dictionaries::gen_types(out, idl)?;

    for definition in idl {
        match definition {
            ast::Definition::Callback(_) => unimplemented!(),
            ast::Definition::Dictionary(ast::Dictionary::NonPartial(_)) => {},
            ast::Definition::Dictionary(ast::Dictionary::Partial(_)) => unimplemented!(),
            ast::Definition::Enum(en) => {},
            ast::Definition::Implements(_) => unimplemented!(),
            ast::Definition::Includes(_) => {}, // FIXME: unimplemented!()
            ast::Definition::Interface(ast::Interface::Callback(_)) => unimplemented!(),
            ast::Definition::Interface(ast::Interface::Partial(interface)) => {
                writeln!(out, "impl {} {{", interface.name)?;
                for member in interface.members.iter() {
                    gen_interface_member(out, idl, &interface.name, member)?;
                }
                writeln!(out, "}}")?;
            },
            ast::Definition::Interface(ast::Interface::NonPartial(interface)) => { // FIXME: unimplemented!()
                writeln!(out, "#[derive(Debug, parity_scale_codec::Encode, parity_scale_codec::Decode)]")?;
                writeln!(out, "pub struct {} {{", interface.name)?;
                writeln!(out, "    inner: u64,")?;
                writeln!(out, "}}")?;
                writeln!(out, "impl {} {{", interface.name)?;
                for member in interface.members.iter() {
                    gen_interface_member(out, idl, &interface.name, member)?;
                }
                writeln!(out, "}}")?;
            },
            ast::Definition::Mixin(_) => {}, // FIXME: unimplemented!()
            ast::Definition::Namespace(_) => unimplemented!(),
            ast::Definition::Typedef(_) => {},
        }
    }

    Ok(())
}

fn gen_interface_member(out: &mut impl Write, idl: &ast::AST, interface_name: &str, member: &ast::InterfaceMember) -> Result<(), io::Error> {
    match member {
        ast::InterfaceMember::Attribute(ast::Attribute::Regular(attribute)) => {
            // FIXME: not implemented
            // TODO: not implemented assert!(attribute.extended_attributes.is_empty());
            /*panic!("{:?}", attribute.type_);
            panic!("{}", attribute.name);*/
        }
        ast::InterfaceMember::Attribute(ast::Attribute::Static(attribute)) => unimplemented!(),
        ast::InterfaceMember::Attribute(ast::Attribute::Stringifier(_)) => unimplemented!(),
        ast::InterfaceMember::Const(member) => {
            assert!(member.extended_attributes.is_empty());
            assert!(!member.nullable);
            let ty = match &member.type_ {
                ast::ConstType::Boolean => unimplemented!(),
                ast::ConstType::Byte => unimplemented!(),
                ast::ConstType::Identifier(id) => format!("{}", id),
                ast::ConstType::Octet => unimplemented!(),
                ast::ConstType::RestrictedDouble => unimplemented!(),
                ast::ConstType::RestrictedFloat => unimplemented!(),
                ast::ConstType::SignedLong => unimplemented!(),
                ast::ConstType::SignedLongLong => unimplemented!(),
                ast::ConstType::SignedShort => unimplemented!(),
                ast::ConstType::UnrestrictedDouble => unimplemented!(),
                ast::ConstType::UnrestrictedFloat => unimplemented!(),
                ast::ConstType::UnsignedLong => unimplemented!(),
                ast::ConstType::UnsignedLongLong => unimplemented!(),
                ast::ConstType::UnsignedShort => unimplemented!(),
            };
            let value = match member.value {
                ast::ConstValue::Null => unimplemented!(),
                ast::ConstValue::BooleanLiteral(true) => format!("true"),
                ast::ConstValue::BooleanLiteral(false) => format!("false"),
                ast::ConstValue::FloatLiteral(_) => unimplemented!(),
                ast::ConstValue::SignedIntegerLiteral(val) => format!("{}", val),
                ast::ConstValue::UnsignedIntegerLiteral(val) => format!("{}", val),
            };
            writeln!(out, "    pub const {}: {} = {};", member.name, ty, value)?;
        },
        ast::InterfaceMember::Iterable(_) => unimplemented!(),
        ast::InterfaceMember::Maplike(_) => unimplemented!(),
        ast::InterfaceMember::Operation(ast::Operation::Regular(op)) => {
            assert!(op.extended_attributes.is_empty());
            if let Some(name) = op.name.as_ref() {
                write!(out, "    pub fn {}(&self", name.to_snake())?;
                for arg in op.arguments.iter() {
                    write!(out, ", {}: {}", arg.name.to_snake(), crate::ty_to_rust(&arg.type_))?;
                }
                let message_answer_ty = message_answer_ty(idl, &op.return_type);
                match &message_answer_ty {
                    MessageAnswerTy::Void => writeln!(out, ") {{ ")?,
                    MessageAnswerTy::Injected(ty) => writeln!(out, ") -> {} {{ ", ty)?,
                    MessageAnswerTy::Promise(ty) => writeln!(out, ") -> impl Future<Output = {}> {{ ", ty)?,
                }
                if let MessageAnswerTy::Injected(_) = message_answer_ty {
                    writeln!(out, "        let return_value = NEXT_OBJECT_ID.fetch_add(1, atomic::Ordering::Relaxed);")?;
                }
                writeln!(out, "        let msg = ffi::WebGPUMessage::{}{} {{", interface_name, name.to_camel())?;
                writeln!(out, "            this: self.inner,")?;
                if let MessageAnswerTy::Injected(_) = message_answer_ty {
                    writeln!(out, "            return_value,")?;
                }
                for arg in op.arguments.iter() {
                    writeln!(out, "            {},", arg.name.to_snake())?;
                }
                writeln!(out, "        }};")?;
                writeln!(out, "        unsafe {{")?;
                match &message_answer_ty {
                    MessageAnswerTy::Void => {
                        writeln!(out, "            redshirt_syscalls_interface::emit_message_without_response(&ffi::INTERFACE, &msg).unwrap();")?;
                    }
                    MessageAnswerTy::Injected(ty) => {
                        writeln!(out, "            redshirt_syscalls_interface::emit_message_without_response(&ffi::INTERFACE, &msg).unwrap();")?;
                        writeln!(out, "            {} {{ inner: return_value }}", ty)?;
                    }
                    MessageAnswerTy::Promise(_) => {
                        writeln!(out, "            redshirt_syscalls_interface::emit_message_with_response(&ffi::INTERFACE, &msg).unwrap()")?;
                    }
                }
                writeln!(out, "        }}")?;
                writeln!(out, "    }}")?;
            } else {
                // TODO: what is that???
            }
        },
        ast::InterfaceMember::Operation(ast::Operation::Special(_)) => unimplemented!(),
        ast::InterfaceMember::Operation(ast::Operation::Static(_)) => unimplemented!(),
        ast::InterfaceMember::Operation(ast::Operation::Stringifier(_)) => unimplemented!(),
        ast::InterfaceMember::Setlike(_) => unimplemented!(),
    }

    Ok(())
}

enum MessageAnswerTy<'a> {
    /// Message is a one-shot operation that doesn't return anything.
    Void,
    /// Message is a one-shot operation that returns a value. We "allocate" the value locally and
    /// pass it with the message to the interface handler.
    Injected(Cow<'a, str>),
    /// Message expects a response.
    Promise(Cow<'a, str>),
}

// TODO: createBufferMapped has bad output
// TODO: also we shouldn't output `ArrayBuffer`, I guess
fn message_answer_ty<'a>(idl: &'a ast::AST, ret_val: &'a ast::ReturnType) -> MessageAnswerTy<'a> {
    match ret_val {
        ast::ReturnType::Void => MessageAnswerTy::Void,
        ast::ReturnType::NonVoid(ty @ ast::Type { kind: ast::TypeKind::Promise(_), .. }) => {
            let inner_ret_val = match &ty.kind {
                ast::TypeKind::Promise(t) => t,
                _ => unreachable!()
            };

            match &**inner_ret_val {
                ast::ReturnType::Void => MessageAnswerTy::Promise(From::from("()")),
                ast::ReturnType::NonVoid(inner_ty) => MessageAnswerTy::Promise(crate::ty_to_rust(inner_ty)),
            }
        },
        ast::ReturnType::NonVoid(ty @ ast::Type { kind: ast::TypeKind::Identifier(_), .. }) => {
            let id = match &ty.kind {
                ast::TypeKind::Identifier(id) => id,
                _ => unreachable!()
            };

            let id_is_interface = idl.iter().any(|def| {
                match def {
                    ast::Definition::Interface(ast::Interface::Partial(interface)) => interface.name == *id,
                    ast::Definition::Interface(ast::Interface::NonPartial(interface)) => interface.name == *id,
                    _ => false,
                }
            });

            if id_is_interface {
                MessageAnswerTy::Injected(From::from(id))
            } else {
                MessageAnswerTy::Promise(crate::ty_to_rust(ty))
            }
        },
        ast::ReturnType::NonVoid(ty) => MessageAnswerTy::Promise(crate::ty_to_rust(ty)),
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
