//! Parsing of the XML definitions file.

use std::io::Read;
use xml::{EventReader, attribute::OwnedAttribute, name::OwnedName, reader::Events, reader::XmlEvent};

/// Successfully-parsed Vulkan registry definitions.
///
/// > **Note**: This only contains the information we need. No need to completely parse
/// >           everything.
#[derive(Debug)]
pub struct VkRegistry {
    /// List of all the Vulkan commands.
    pub commands: Vec<VkCommand>,
}

/// Successfully-parsed Vulkan command definition.
#[derive(Debug)]
pub struct VkCommand {

}

/// Parses the file `vk.xml` from the given source. Assumes that everything is well-formed and
/// that no error happens.
pub fn parse(source: impl Read) -> VkRegistry {
    let mut events_source = EventReader::new(source).into_iter();

    match events_source.next() {
        Some(Ok(XmlEvent::StartDocument { .. })) => {},
        ev => panic!("Unexpected: {:?}", ev)
    }

    let registry = match events_source.next() {
        Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "registry") =>
            parse_registry(&mut events_source),
        ev => panic!("Unexpected: {:?}", ev)
    };

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::EndDocument { .. })) => break,
            Some(Ok(XmlEvent::Whitespace(..))) => {},
            ev => panic!("Unexpected: {:?}", ev)
        }
    }

    match events_source.next() {
        None => return registry,
        ev => panic!("Unexpected: {:?}", ev)
    }
}

fn parse_registry(events_source: &mut Events<impl Read>) -> VkRegistry {
    let mut out = VkRegistry {
        commands: Vec::new(),
    };

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "types") =>
                parse_types(events_source),
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "commands") => {
                let commands = parse_commands(events_source);
                assert!(out.commands.is_empty());
                out.commands = commands;
            },

            // We actually don't care what enum values are.
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "enums") =>
                advance_until_elem_end(events_source, &name),

            // Other things we don't care about.
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "comment") =>
                advance_until_elem_end(events_source, &name),
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "platforms") =>
                advance_until_elem_end(events_source, &name),
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "tags") =>
                advance_until_elem_end(events_source, &name),
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "feature") =>
                advance_until_elem_end(events_source, &name),
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "extensions") =>
                advance_until_elem_end(events_source, &name),

            Some(Ok(XmlEvent::EndElement { .. })) => return out,
            Some(Ok(XmlEvent::CData(..))) |
            Some(Ok(XmlEvent::Comment(..))) |
            Some(Ok(XmlEvent::Characters(..))) |
            Some(Ok(XmlEvent::Whitespace(..))) => {},
            ev => panic!("Unexpected; probably because unimplemented: {:?}", ev),      // TODO: turn into "Unexpected" once everything is implemented
        }
    }
}

/// Call this function right after finding a `StartElement` with the name `types`. This function
/// parses the content of the element.
fn parse_types(events_source: &mut Events<impl Read>) {
    loop {
        match events_source.next() {
            // TODO: implement
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "type") =>
                advance_until_elem_end(events_source, &name),
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "comment") =>
                advance_until_elem_end(events_source, &name),
            Some(Ok(XmlEvent::EndElement { .. })) => return,
            Some(Ok(XmlEvent::CData(..))) |
            Some(Ok(XmlEvent::Comment(..))) |
            Some(Ok(XmlEvent::Characters(..))) |
            Some(Ok(XmlEvent::Whitespace(..))) => {},
            ev => panic!("Unexpected: {:?}", ev),
        }
    }
}

/// Call this function right after finding a `StartElement` with the name `commands`. This
/// function parses the content of the element.
fn parse_commands(events_source: &mut Events<impl Read>) -> Vec<VkCommand> {
    let mut out = Vec::new();

    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "command") =>
                out.push(parse_command(events_source)),
            Some(Ok(XmlEvent::StartElement { name, .. })) if name_equals(&name, "comment") =>
                advance_until_elem_end(events_source, &name),
            Some(Ok(XmlEvent::EndElement { .. })) => return out,
            Some(Ok(XmlEvent::CData(..))) |
            Some(Ok(XmlEvent::Comment(..))) |
            Some(Ok(XmlEvent::Characters(..))) |
            Some(Ok(XmlEvent::Whitespace(..))) => {},
            ev => panic!("Unexpected: {:?}", ev),
        }
    }
}

/// Call this function right after finding a `StartElement` with the name `command`. This
/// function parses the content of the element.
fn parse_command(events_source: &mut Events<impl Read>) -> VkCommand {
    // TODO:
    advance_until_elem_end(events_source, &"command".parse().unwrap());
    VkCommand {}
}

/// Advances the `events_source` until a corresponding `EndElement` with the given `elem` is found.
///
/// Call this function if you find a `StartElement` whose content you don't care about.
fn advance_until_elem_end(events_source: &mut Events<impl Read>, elem: &OwnedName) {
    loop {
        match events_source.next() {
            Some(Ok(XmlEvent::StartElement { name, .. })) => advance_until_elem_end(events_source, &name),
            Some(Ok(XmlEvent::EndElement { name })) if &name == elem => return,
            Some(Ok(XmlEvent::CData(..))) |
            Some(Ok(XmlEvent::Comment(..))) |
            Some(Ok(XmlEvent::Characters(..))) |
            Some(Ok(XmlEvent::Whitespace(..))) => {},
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
    list.iter().find(|a| name_equals(&a.name, name)).map(|a| a.value.as_str())
}
