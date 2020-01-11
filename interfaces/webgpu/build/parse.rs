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

use webidl::ast;

/// Generate the AST of the WebIDL specs.
pub fn gen_parsed_idl() -> ast::AST {
    let idl = gen_unparsed_idl();
    // TODO: specs have been modified because of https://github.com/gpuweb/gpuweb/issues/533
    // TODO: specs have also been modified to remove a duplicate setBindGroup that was causing issues
    // TODO: specs have also been modified to turn an attribute into a regular function, again for convenience
    match webidl::parse_string(&idl) {
        Ok(ast) => ast,
        Err(err) => {
            let lexing = webidl::Lexer::new(&idl).collect::<Vec<_>>();
            let lexing = lexing
                .into_iter()
                .map(|v| format!("{:?}", v))
                .collect::<Vec<_>>()
                .join("\n");
            panic!("Parse error: {}\nAST: {}\nLexing: {}\n", err, idl, lexing);
        }
    }
}

/// Generate the WebIDL specs as single string.
fn gen_unparsed_idl() -> String {
    // See https://github.com/gpuweb/gpuweb/blob/master/spec/extract-idl.py
    let idl_start = regex::Regex::new(r#"<script .*type=['"]?idl"#).unwrap();
    let idl_stop = regex::Regex::new(r#"</script>"#).unwrap();

    let mut recording = false;
    let mut idl_lines = Vec::new();

    for line in include_str!("idl.bs").lines() {
        if idl_start.is_match(line) {
            assert!(!recording);
            recording = true;
        } else if idl_stop.is_match(line) {
            assert!(recording);
            recording = false;
        } else if recording {
            idl_lines.push(line);
        }
    }

    idl_lines.join("\n")
}
