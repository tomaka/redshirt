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
use std::{
    borrow::Cow,
    io::{self, Write},
};
use webidl::ast;

pub fn gen_ffi(out: &mut impl Write, idl: &ast::AST) -> Result<(), io::Error> {
    writeln!(out, "#[derive(Debug, Encode, Decode)]")?;
    writeln!(out, "pub enum WebGPUMessage {{")?;
    for definition in idl {
        match definition {
            ast::Definition::Interface(ast::Interface::Partial(interface)) => {
                for member in interface.members.iter() {
                    gen_interface_member(out, idl, &interface.name, member)?;
                }
            }
            ast::Definition::Interface(ast::Interface::NonPartial(interface)) => {
                // TODO: these destroy messages need some brainstorming
                /*write!(out, "    Destroy{} {{ ", interface.name)?;
                write!(out, "this: {} ", interface.name)?;
                writeln!(out, "}},")?;*/

                for member in interface.members.iter() {
                    gen_interface_member(out, idl, &interface.name, member)?;
                }
            }
            ast::Definition::Includes(include) => {
                assert!(include.extended_attributes.is_empty());
                for def in idl {
                    match def {
                        ast::Definition::Mixin(ast::Mixin::NonPartial(mixin))
                            if mixin.name == include.includee =>
                        {
                            for member in mixin.members.iter() {
                                gen_mixin_member(out, idl, &include.includer, member)?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    writeln!(out, "}}")?;

    for definition in idl {
        if let ast::Definition::Interface(ast::Interface::NonPartial(interface)) = definition {
            writeln!(out, "type {} = u64;", interface.name)?;
        }
    }

    crate::dictionaries::gen_types(out, idl)?;
    crate::dictionaries::gen_enums(out, idl)?;
    Ok(())
}

/// > Note: `member` doesn't necessarily need to belong to `interface_name`. This is useful for
/// >       `includes` definitions.
fn gen_interface_member(
    out: &mut impl Write,
    idl: &ast::AST,
    interface_name: &str,
    member: &ast::InterfaceMember,
) -> Result<(), io::Error> {
    match member {
        ast::InterfaceMember::Iterable(_) => unimplemented!(),
        ast::InterfaceMember::Maplike(_) => unimplemented!(),
        ast::InterfaceMember::Operation(op) => gen_interface_op(out, idl, interface_name, op)?,
        ast::InterfaceMember::Setlike(_) => unimplemented!(),
        _ => {} // FIXME:
    }

    Ok(())
}

/// > Note: `member` doesn't necessarily need to belong to `interface_name`. This is useful for
/// >       `includes` definitions.
fn gen_mixin_member(
    out: &mut impl Write,
    idl: &ast::AST,
    interface_name: &str,
    member: &ast::MixinMember,
) -> Result<(), io::Error> {
    match member {
        ast::MixinMember::Operation(op) => gen_interface_op(out, idl, interface_name, op)?,
        _ => {} // FIXME:
    }

    Ok(())
}

/// > Note: `member` doesn't necessarily need to belong to `interface_name`. This is useful for
/// >       `includes` definitions.
fn gen_interface_op(
    out: &mut impl Write,
    idl: &ast::AST,
    interface_name: &str,
    op: &ast::Operation,
) -> Result<(), io::Error> {
    match op {
        ast::Operation::Regular(op) => {
            assert!(op.extended_attributes.is_empty());
            if let Some(name) = op.name.as_ref() {
                if let Some(message_answer_ty) = message_answer_ty(idl, &op.return_type) {
                    writeln!(out, "    // Answer: {}", message_answer_ty)?;
                }
                write!(out, "    {}{} {{ ", interface_name, name.to_camel())?;
                write!(out, "this: {}, ", interface_name)?;
                //write!(out, "this: {}, ", interface_name)?;
                if let Some(return_value_to_pass) = return_value_to_pass(idl, &op.return_type) {
                    write!(out, "return_value: {}, ", return_value_to_pass)?;
                }
                for arg in op.arguments.iter() {
                    write!(
                        out,
                        "{}: {}, ",
                        arg.name.to_snake(),
                        crate::ty_to_rust(&arg.type_)
                    )?;
                }
                writeln!(out, "}},")?;
            } else {
                // TODO: what is that???
            }
        }
        ast::Operation::Special(_) => unimplemented!(),
        ast::Operation::Static(_) => unimplemented!(),
        ast::Operation::Stringifier(_) => unimplemented!(),
    }

    Ok(())
}

fn return_value_to_pass(idl: &ast::AST, ret_val: &ast::ReturnType) -> Option<Cow<'static, str>> {
    match ret_val {
        ast::ReturnType::Void => None,
        ast::ReturnType::NonVoid(
            ty @ ast::Type {
                kind: ast::TypeKind::Identifier(_),
                ..
            },
        ) => {
            let id = match &ty.kind {
                ast::TypeKind::Identifier(id) => id,
                _ => unreachable!(),
            };

            let id_is_interface = idl.iter().any(|def| match def {
                ast::Definition::Interface(ast::Interface::Partial(interface)) => {
                    interface.name == *id
                }
                ast::Definition::Interface(ast::Interface::NonPartial(interface)) => {
                    interface.name == *id
                }
                _ => false,
            });

            if id_is_interface {
                Some(From::from("u64"))
            } else {
                None
            }
        }
        ast::ReturnType::NonVoid(_) => None,
    }
}

// TODO: createBufferMapped has bad output
// TODO: also we shouldn't output `ArrayBuffer`, I guess
fn message_answer_ty(idl: &ast::AST, ret_val: &ast::ReturnType) -> Option<Cow<'static, str>> {
    match ret_val {
        ast::ReturnType::Void => None,
        ast::ReturnType::NonVoid(
            ty @ ast::Type {
                kind: ast::TypeKind::Promise(_),
                ..
            },
        ) => {
            let inner_ret_val = match &ty.kind {
                ast::TypeKind::Promise(t) => t,
                _ => unreachable!(),
            };

            match &**inner_ret_val {
                ast::ReturnType::Void => Some(From::from("()")),
                ast::ReturnType::NonVoid(inner_ty) => Some(crate::ty_to_rust(inner_ty)),
            }
        }
        ast::ReturnType::NonVoid(
            ty @ ast::Type {
                kind: ast::TypeKind::Identifier(_),
                ..
            },
        ) => {
            let id = match &ty.kind {
                ast::TypeKind::Identifier(id) => id,
                _ => unreachable!(),
            };

            let id_is_interface = idl.iter().any(|def| match def {
                ast::Definition::Interface(ast::Interface::Partial(interface)) => {
                    interface.name == *id
                }
                ast::Definition::Interface(ast::Interface::NonPartial(interface)) => {
                    interface.name == *id
                }
                _ => false,
            });

            if id_is_interface {
                None
            } else {
                Some(crate::ty_to_rust(ty))
            }
        }
        ast::ReturnType::NonVoid(ty) => Some(crate::ty_to_rust(ty)),
    }
}
