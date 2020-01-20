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

//! Parsing of the XML definitions file.

use std::{collections::HashMap, io::Read};
use xml::{
    attribute::OwnedAttribute, name::OwnedName, reader::Events, reader::XmlEvent, EventReader,
};

/// Successfully-parsed Vulkan registry definitions.
///
/// > **Note**: This only contains the information we need. No need to completely parse
/// >           everything.
#[derive(Debug, Clone)]
pub struct VkRegistry {
    /// List of all the Vulkan commands.
    pub commands: Vec<VkCommand>,
    /// Type definitions.
    pub type_defs: HashMap<String, VkTypeDef>,
    /// Enum values.
    pub enums: HashMap<String, String>,
}

/// A type definition of the Vulkan API.
#[derive(Debug, Clone)]
pub enum VkTypeDef {
    Enum,
    Bitmask,
    DispatchableHandle,
    NonDispatchableHandle,
    Struct { fields: Vec<(VkType, String)> },
    Union { fields: Vec<(VkType, String)> },
}

/// Successfully-parsed Vulkan command definition.
#[derive(Debug, Clone)]
pub struct VkCommand {
    /// Name of the Vulkan function, with the `vk` prefix.
    pub name: String,
    /// Return type of the function.
    pub ret_ty: VkType,
    /// List of parameters of the function, with their type and name.
    pub params: Vec<(VkType, String)>,
}

/// Successfully-parsed Vulkan type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VkType {
    /// A single type identifier. Can be either a primitive C type (such as `void`, `uin32_t`, or
    /// `xcb_window_t`), or a type defined in the registry. Never contains any pointer or array.
    Ident(String),

    /// Pointer to some memory location containing a certain number of elements of the given type.
    MutPointer(Box<VkType>, VkTypePtrLen),

    /// Pointer to some memory location containing a certain number of elements of the given type.
    ConstPointer(Box<VkType>, VkTypePtrLen),

    /// Array of fixed size. The size is given by the second parameter and can be either a
    /// constant numeric value (for example `2`), or a constant from the registry (for example
    /// `VK_MAX_DESCRIPTION_SIZE`).
    Array(Box<VkType>, String),
}

/// Number of elements in a memory location indicated with a pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VkTypePtrLen {
    /// The pointer points to a single element.
    One,

    /// The pointer points to a list of elements that ends when a `0` is reached.
    /// Typically used for C-style strings.
    /// For example a `const char*` is represented as
    /// `VkType::ConstPointer(VkType::Ident("char"), VkTypePtrLen::NullTerminated)`.
    NullTerminated,

    /// The size of the list is given by some runtime Rust expression. The expression might require
    /// knowing the value of an other field of the struct or list of parameters that this type is
    /// contained it.
    ///
    /// In order to obtain a Rust expression that contains the length, transform `other_field` into
    /// an expression containing the value of `other_field`, and prepend `before_other_field` and
    /// append `after_other_field`.
    ///
    /// `other_field` might be a list, in which case the first element is the other field, and each
    /// subsequent element is a subfield the previous one.
    ///
    /// For example `before_other_field` might be `"("` and `after_other_field` might be `" / 4)"`.
    OtherField {
        before_other_field: String,
        other_field: Vec<String>,
        after_other_field: String,
    },
}

/// Finds the type of a subfield of the list of fields.
///
/// The registry must be passed in order to look up the definition of structs.
// TODO: better description
pub fn gen_path_subfield_in_list<'b>(
    fields: &'b [(VkType, String)],
    registry: &'b VkRegistry,
    subfields: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<(String, &'b VkType)> {
    let mut subfields = subfields.into_iter();
    let first = subfields.next().unwrap();
    let mut path = String::new();
    path.push_str(first.as_ref());
    let mut ty = fields
        .iter()
        .find(|(_, n)| n == first.as_ref())
        .map(|(t, _)| t)?;
    while let Some(next) = subfields.next() {
        path = ty.gen_deref_expr(&path);
        if let VkTypeDef::Struct { fields } = registry
            .type_defs
            .get(ty.derefed_type().as_ident().unwrap())
            .unwrap()
        {
            ty = fields
                .iter()
                .find(|(_, n)| n == next.as_ref())
                .map(|(t, _)| t)?;
            path.push_str(".r#");
            path.push_str(next.as_ref());
        } else {
            return None;
        }
    }
    Some((path, ty))
}

/// Finds the type of a subfield of the list of fields.
///
/// The registry must be passed in order to look up the definition of structs.
pub fn find_subfield_in_list<'b>(
    fields: &'b [(VkType, String)],
    registry: &'b VkRegistry,
    subfields: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<&'b VkType> {
    gen_path_subfield_in_list(fields, registry, subfields).map(|r| r.1)
}

impl VkType {
    /// If `self` is a pointer, generates an expression that dereferences `expr` until it is
    /// a plain value.
    pub fn gen_deref_expr(&self, expr: &str) -> String {
        match self {
            VkType::MutPointer(t, _) => t.gen_deref_expr(&format!("(*{})", expr)),
            VkType::ConstPointer(t, _) => t.gen_deref_expr(&format!("(*{})", expr)),
            _ => expr.to_owned(),
        }
    }

    /// If `self` is a pointer, returns the dereferenced type. Does that recursively.
    pub fn derefed_type(&self) -> &VkType {
        match self {
            VkType::MutPointer(t, _) => t.derefed_type(),
            VkType::ConstPointer(t, _) => t.derefed_type(),
            t => t,
        }
    }

    /// If `self` is an `Ident`, returns the identifier.
    pub fn as_ident(&self) -> Option<&str> {
        match self {
            VkType::Ident(s) => Some(&s),
            _ => None,
        }
    }
}

impl VkTypeDef {
    /// If `self` is a `Struct`, returns the type of the given subfield.
    pub fn resolve_subfield_ty(&self, subfield: &str) -> Option<&VkType> {
        match self {
            VkTypeDef::Struct { fields } => {
                if let Some(elem) = fields.iter().find(|(_, n)| n == subfield) {
                    Some(&elem.0)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Parses the file `vk.xml` from the given source. Assumes that everything is well-formed and
/// that no error happens.
pub fn parse(source: impl Read) -> VkRegistry {
    let mut events_source = EventReader::new(source).into_iter();

    match events_source.next() {
        Some(Ok(XmlEvent::StartDocument { .. })) => {}
        ev => panic!("Unexpected: {:?}", ev),
    }

    let registry = match events_source.next() {
        Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "registry") => {
            parse_registry(&mut events_source)
        }
        ev => panic!("Unexpected: {:?}", ev),
    };

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::EndDocument { .. })) => break,
            Some(Ok(XmlEvent::Whitespace(..))) => {}
            ev => panic!("Unexpected: {:?}", ev),
        }
    }

    match events_source.next() {
        None => return registry,
        ev => panic!("Unexpected: {:?}", ev),
    }
}

// # About parsing
//
// The XML library we're using proposes a streaming compilation API. What this means it that it
// parses the XML code and feeds us with parsing events such as `StartElement`, `EndElement`
// or `Characters`.
//
// The content of this module accomodates this. The various functions below expect as input
// a `&mut Events` (where `Events` is an iterator) and advance the iterator until they leave
// the current element. If anything unexpected is encountered on the way, everything stops and a
// panic immediately happens.
//

fn parse_registry(events_source: &mut Events<impl Read>) -> VkRegistry {
    let mut out = VkRegistry {
        commands: Vec::new(),
        type_defs: HashMap::new(),
        enums: HashMap::new(),
    };

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "types") => {
                let type_defs = parse_types(events_source);
                assert!(out.type_defs.is_empty());
                out.type_defs = type_defs;
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "commands") => {
                let commands = parse_commands(events_source);
                assert!(out.commands.is_empty());
                out.commands = commands;
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "enums") => {
                for (name, value) in parse_enums(events_source) {
                    let _prev_val = out.enums.insert(name.clone(), value);
                    assert!(_prev_val.is_none(), "Duplicate value for {:?}", name);
                }
            }

            // Other things we don't care about.
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "comment") => {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "platforms") => {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "tags") => {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "feature") => {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "extensions") => {
                advance_until_elem_end(events_source, &name)
            }

            Some(Ok(XmlEvent::EndElement { .. })) => {
                assert!(!out.commands.is_empty());
                assert!(!out.type_defs.is_empty());
                return out;
            }
            Some(Ok(XmlEvent::CData(..)))
            | Some(Ok(XmlEvent::Comment(..)))
            | Some(Ok(XmlEvent::Characters(..)))
            | Some(Ok(XmlEvent::Whitespace(..))) => {}
            ev => panic!("Unexpected: {:?}", ev),
        }
    }
}

/// Call this function right after finding a `StartElement` with the name `types`. This function
/// parses the content of the element.
fn parse_types(events_source: &mut Events<impl Read>) -> HashMap<String, VkTypeDef> {
    let mut out = HashMap::new();

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::StartElement {
                name, attributes, ..
            })) if name_equals(&name, "type") => {
                if let Some((name, ty)) = parse_type(events_source, attributes) {
                    if !name.is_empty() {
                        // TODO: shouldn't be there; find the bug
                        let _prev_val = out.insert(name.clone(), ty);
                        assert!(_prev_val.is_none(), "Duplicate value for {:?}", name);
                    }
                }
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "comment") => {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::EndElement { name, .. })) => {
                assert!(name_equals(&name, "types"));
                return out;
            }
            Some(Ok(XmlEvent::CData(..)))
            | Some(Ok(XmlEvent::Comment(..)))
            | Some(Ok(XmlEvent::Characters(..)))
            | Some(Ok(XmlEvent::Whitespace(..))) => {}
            ev => panic!("Unexpected: {:?}", ev),
        }
    }
}

/// Call this function right after finding a `StartElement` with the name `type`. This
/// function parses the content of the element.
fn parse_type(
    events_source: &mut Events<impl Read>,
    attributes: Vec<OwnedAttribute>,
) -> Option<(String, VkTypeDef)> {
    match find_attr(&attributes, "category") {
        Some("enum") => {
            let name = find_attr(&attributes, "name").unwrap().to_owned();
            advance_until_elem_end(events_source, &"type".parse().unwrap());
            Some((name, VkTypeDef::Enum))
        }
        Some("bitmask") => {
            let (_, name) = parse_ty_name(events_source, attributes);
            Some((name, VkTypeDef::Bitmask))
        }
        Some("include") | Some("define") | Some("basetype") => {
            advance_until_elem_end(events_source, &"type".parse().unwrap());
            None
        }
        Some("handle") => {
            let (ty, name) = parse_ty_name(events_source, attributes.clone());
            if ty == VkType::Ident("VK_DEFINE_HANDLE".to_owned()) {
                Some((name, VkTypeDef::DispatchableHandle))
            } else if ty == VkType::Ident("VK_DEFINE_NON_DISPATCHABLE_HANDLE".to_owned()) {
                Some((name, VkTypeDef::NonDispatchableHandle))
            } else if find_attr(&attributes, "alias").is_some() {
                None
            } else {
                panic!("Unknown handle type: {:?} for {:?}", ty, name)
            }
        }
        Some("funcpointer") => {
            // We deliberately ignore function pointers, and manually generate their definitions.
            advance_until_elem_end(events_source, &"type".parse().unwrap());
            None
        }
        Some("union") => {
            let name = find_attr(&attributes, "name").unwrap().to_owned();
            let mut fields = Vec::new();

            loop {
                match events_source.next() {
                    Some(Ok(XmlEvent::StartElement {
                        name, attributes, ..
                    })) if name_equals(&name, "member") => {
                        fields.push(parse_ty_name(events_source, attributes));
                    }
                    Some(Ok(XmlEvent::StartElement { name, .. }))
                        if name_equals(&name, "comment") =>
                    {
                        advance_until_elem_end(events_source, &name)
                    }
                    Some(Ok(XmlEvent::EndElement { .. })) => break,
                    Some(Ok(XmlEvent::CData(..)))
                    | Some(Ok(XmlEvent::Comment(..)))
                    | Some(Ok(XmlEvent::Characters(..)))
                    | Some(Ok(XmlEvent::Whitespace(..))) => {}
                    ev => panic!("Unexpected: {:?}", ev),
                }
            }

            Some((name, VkTypeDef::Union { fields }))
        }
        Some("struct") => {
            let name = find_attr(&attributes, "name").unwrap().to_owned();
            let mut fields = Vec::new();

            loop {
                match events_source.next() {
                    Some(Ok(XmlEvent::StartElement {
                        name, attributes, ..
                    })) if name_equals(&name, "member") => {
                        fields.push(parse_ty_name(events_source, attributes));
                    }
                    Some(Ok(XmlEvent::StartElement { name, .. }))
                        if name_equals(&name, "comment") =>
                    {
                        advance_until_elem_end(events_source, &name)
                    }
                    Some(Ok(XmlEvent::EndElement { .. })) => break,
                    Some(Ok(XmlEvent::CData(..)))
                    | Some(Ok(XmlEvent::Comment(..)))
                    | Some(Ok(XmlEvent::Characters(..)))
                    | Some(Ok(XmlEvent::Whitespace(..))) => {}
                    ev => panic!("Unexpected: {:?}", ev),
                }
            }

            Some((name, VkTypeDef::Struct { fields }))
        }
        None if find_attr(&attributes, "requires").is_some() => {
            advance_until_elem_end(events_source, &"type".parse().unwrap());
            None
        }
        None if find_attr(&attributes, "name") == Some("int") => {
            advance_until_elem_end(events_source, &"type".parse().unwrap());
            None
        }
        cat => panic!(
            "Unexpected type category: {:?} with attrs {:?}",
            cat, attributes
        ),
    }
}

/// Call this function right after finding a `StartElement` with the name `enums`. This function
/// parses the content of the element.
fn parse_enums(events_source: &mut Events<impl Read>) -> HashMap<String, String> {
    let mut out = HashMap::new();

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::StartElement {
                name, attributes, ..
            })) if name_equals(&name, "enum") => {
                if let Some((name, value)) = parse_enum(events_source, attributes) {
                    let _prev_val = out.insert(name.clone(), value);
                    assert!(_prev_val.is_none(), "Duplicate value for {:?}", name);
                }
            }

            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "comment") => {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "unused") => {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::EndElement { name, .. })) => {
                assert!(name_equals(&name, "enums"));
                return out;
            }
            Some(Ok(XmlEvent::CData(..)))
            | Some(Ok(XmlEvent::Comment(..)))
            | Some(Ok(XmlEvent::Characters(..)))
            | Some(Ok(XmlEvent::Whitespace(..))) => {}
            ev => panic!("Unexpected: {:?}", ev),
        }
    }
}

/// Call this function right after finding a `StartElement` with the name `enum`. This
/// function parses the content of the element.
fn parse_enum(
    events_source: &mut Events<impl Read>,
    attributes: Vec<OwnedAttribute>,
) -> Option<(String, String)> {
    let name = find_attr(&attributes, "name").unwrap().to_owned();

    let value = if let Some(value) = find_attr(&attributes, "value") {
        value.to_owned()
    } else if let Some(alias) = find_attr(&attributes, "alias") {
        alias.to_owned()
    } else if let Some(bitpos) = find_attr(&attributes, "bitpos") {
        format!("2 << {}", bitpos)
    } else {
        panic!("Can't figure out enum value: {:?}", attributes);
    };

    advance_until_elem_end(events_source, &"enum".parse().unwrap());
    Some((name, value))
}

/// Call this function right after finding a `StartElement` with the name `commands`. This
/// function parses the content of the element.
fn parse_commands(events_source: &mut Events<impl Read>) -> Vec<VkCommand> {
    let mut out = Vec::new();

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::StartElement {
                name, attributes, ..
            })) if name_equals(&name, "command") => {
                if let Some(cmd) = parse_command(events_source, attributes) {
                    out.push(cmd);
                }
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "comment") => {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::EndElement { .. })) => return out,
            Some(Ok(XmlEvent::CData(..)))
            | Some(Ok(XmlEvent::Comment(..)))
            | Some(Ok(XmlEvent::Characters(..)))
            | Some(Ok(XmlEvent::Whitespace(..))) => {}
            ev => panic!("Unexpected: {:?}", ev),
        }
    }
}

/// Call this function right after finding a `StartElement` with the name `command`. This
/// function parses the content of the element.
fn parse_command(
    events_source: &mut Events<impl Read>,
    attributes: Vec<OwnedAttribute>,
) -> Option<VkCommand> {
    let mut out = VkCommand {
        name: String::new(),
        ret_ty: VkType::Ident(String::new()),
        params: Vec::new(),
    };

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::StartElement {
                name, attributes, ..
            })) if name_equals(&name, "proto") => {
                let (ret_ty, f_name) = parse_ty_name(events_source, attributes);
                out.name = f_name;
                out.ret_ty = ret_ty;
            }

            Some(Ok(XmlEvent::StartElement {
                name, attributes, ..
            })) if name_equals(&name, "param") => {
                out.params.push(parse_ty_name(events_source, attributes));
            }

            Some(Ok(XmlEvent::StartElement { name, .. }))
                if name_equals(&name, "implicitexternsyncparams") =>
            {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::EndElement { .. })) => break,
            Some(Ok(XmlEvent::CData(..)))
            | Some(Ok(XmlEvent::Comment(..)))
            | Some(Ok(XmlEvent::Characters(..)))
            | Some(Ok(XmlEvent::Whitespace(..))) => {}
            ev => panic!("Unexpected: {:?}", ev),
        }
    }

    if out.name.is_empty() || out.ret_ty == VkType::Ident(String::new()) {
        // TODO: aliases must also be returned somehow
        assert!(find_attr(&attributes, "alias").is_some());
        return None;
    }

    Some(out)
}

/// Call this function right after finding a `StartElement`. This function parses the content of
/// the element and expects a single `<type>` tag and a single `<name>` tag. It returns the type
/// and the name.
fn parse_ty_name(
    events_source: &mut Events<impl Read>,
    attributes: Vec<OwnedAttribute>,
) -> (VkType, String) {
    let mut ret_ty_out = String::new();
    let mut name_out = String::new();
    let mut enum_content = String::new();
    let len_attr = find_attr(&attributes, "len");

    let mut white_spaces = String::new();

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "name") => {
                name_out = expect_characters_elem(events_source)
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "type") => {
                ret_ty_out = expect_characters_elem(events_source)
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "enum") => {
                enum_content = expect_characters_elem(events_source)
            }
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "comment") => {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::EndElement { .. })) => break,
            Some(Ok(XmlEvent::CData(s))) => white_spaces.push_str(&s),
            Some(Ok(XmlEvent::Comment(s))) => white_spaces.push_str(&s),
            Some(Ok(XmlEvent::Characters(s))) => white_spaces.push_str(&s),
            Some(Ok(XmlEvent::Whitespace(s))) => white_spaces.push_str(&s),
            ev => panic!("Unexpected: {:?}", ev),
        }
    }

    let ret_ty = if white_spaces.contains("*") {
        // TODO: we assume that there's no weird combination such as mut pointers to const pointers
        if let Some(len) = len_attr {
            let mut ty_out = VkType::Ident(ret_ty_out);
            for elem in len.split(',').rev() {
                let len = if elem == "null-terminated" {
                    VkTypePtrLen::NullTerminated
                } else if elem
                    == r#"latexmath:[\lceil{\mathit{rasterizationSamples} \over 32}\rceil]"#
                {
                    VkTypePtrLen::OtherField {
                        before_other_field: "(".to_owned(),
                        other_field: vec!["rasterizationSamples".to_owned()],
                        after_other_field: " + 31) / 32".to_owned(),
                    }
                } else if elem == r#"latexmath:[\textrm{codeSize} \over 4]"# {
                    VkTypePtrLen::OtherField {
                        before_other_field: "".to_owned(),
                        other_field: vec!["codeSize".to_owned()],
                        after_other_field: " / 4".to_owned(),
                    }
                } else {
                    // If `altlen` is something, then this is likely a mathematical expression
                    // that needs to be hardcoded similar to the ones above.
                    assert!(
                        find_attr(&attributes, "altlen").is_none(),
                        "Field runtime length might have to be hardcoded: {:?}",
                        len
                    );
                    VkTypePtrLen::OtherField {
                        before_other_field: "".to_owned(),
                        other_field: elem.split("::").map(|v| v.to_owned()).collect(),
                        after_other_field: "".to_owned(),
                    }
                };

                if white_spaces.contains("const") {
                    ty_out = VkType::ConstPointer(Box::new(ty_out), len);
                } else {
                    ty_out = VkType::MutPointer(Box::new(ty_out), len);
                }
            }
            ty_out
        } else {
            if white_spaces.contains("const") {
                VkType::ConstPointer(Box::new(VkType::Ident(ret_ty_out)), VkTypePtrLen::One)
            } else {
                VkType::MutPointer(Box::new(VkType::Ident(ret_ty_out)), VkTypePtrLen::One)
            }
        }
    } else {
        assert!(len_attr.is_none());

        if white_spaces.contains("[") {
            if enum_content.is_empty() {
                // TODO: hard-coded :-/
                if white_spaces.contains("[2]") {
                    VkType::Array(Box::new(VkType::Ident(ret_ty_out)), "2".into())
                } else if white_spaces.contains("[3]") {
                    VkType::Array(Box::new(VkType::Ident(ret_ty_out)), "3".into())
                } else if white_spaces.contains("[4]") {
                    VkType::Array(Box::new(VkType::Ident(ret_ty_out)), "4".into())
                } else {
                    panic!()
                }
            } else {
                VkType::Array(Box::new(VkType::Ident(ret_ty_out)), enum_content)
            }
        } else {
            VkType::Ident(ret_ty_out)
        }
    };

    (ret_ty, name_out)
}

/// Advances the `events_source` until a corresponding `EndElement` with the given `elem` is found.
///
/// Call this function if you find a `StartElement` whose content you don't care about.
fn advance_until_elem_end(events_source: &mut Events<impl Read>, elem: &OwnedName) {
    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::StartElement { name, .. })) => {
                advance_until_elem_end(events_source, &name)
            }
            Some(Ok(XmlEvent::EndElement { name })) if &name == elem => return,
            Some(Ok(XmlEvent::CData(..)))
            | Some(Ok(XmlEvent::Comment(..)))
            | Some(Ok(XmlEvent::Characters(..)))
            | Some(Ok(XmlEvent::Whitespace(..))) => {}
            ev => panic!("Unexpected: {:?}", ev),
        }
    }
}

/// Call this function if you find a `StartElement`. This function will grab any character within
/// the element and will return when it encounters the corresponding `EndElement`. Any other
/// `StartElement` within will trigger a panic.
fn expect_characters_elem(events_source: &mut Events<impl Read>) -> String {
    let mut out = String::new();

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::EndElement { .. })) => return out,
            Some(Ok(XmlEvent::CData(s))) => out.push_str(&s),
            Some(Ok(XmlEvent::Comment(s))) => out.push_str(&s),
            Some(Ok(XmlEvent::Characters(s))) => out.push_str(&s),
            Some(Ok(XmlEvent::Whitespace(s))) => out.push_str(&s),
            ev => panic!("Unexpected: {:?}", ev),
        }
    }
}

/// Checks whether an `OwnedName` matches the expected name.
fn name_equals(name: &OwnedName, expected: &str) -> bool {
    name.namespace.is_none() && name.prefix.is_none() && name.local_name == expected
}

/// Find an attribute value in the list.
fn find_attr<'a>(list: &'a [OwnedAttribute], name: &str) -> Option<&'a str> {
    list.iter()
        .find(|a| name_equals(&a.name, name))
        .map(|a| a.value.as_str())
}
