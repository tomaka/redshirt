// Copyright (C) 2019-2021  Pierre Krieger
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

// TODO: main reference about API usage is https://github.com/bytecodealliance/wasmtime/blob/main/crates/environ/src/module_environ.rs

use cranelift_codegen::*;

pub fn build(wasm_bytecode: &[u8]) -> Result<(), wasmparser::BinaryReaderError> {
    for event in wasmparser::Parser::new(0).parse_all(wasm_bytecode) {
        match event? {
            wasmparser::Payload::Version { num, range } => {}
            wasmparser::Payload::StartSection { range, func } => {}
            wasmparser::Payload::End => {}
            wasmparser::Payload::AliasSection(_) => {}
            wasmparser::Payload::CodeSectionEntry(_) => {}
            wasmparser::Payload::DataSection(_) => {}
            wasmparser::Payload::DataCountSection { count, range } => {}
            wasmparser::Payload::ElementSection(_) => {}
            wasmparser::Payload::ExportSection(_) => {}
            wasmparser::Payload::FunctionSection(_) => {}
            wasmparser::Payload::GlobalSection(_) => {}
            wasmparser::Payload::ImportSection(_) => {}
            wasmparser::Payload::InstanceSection(_) => {}
            wasmparser::Payload::MemorySection(_) => {}
            wasmparser::Payload::TableSection(_) => {}
            wasmparser::Payload::TagSection(_) => {}
            wasmparser::Payload::TypeSection(_) => {}
            wasmparser::Payload::CustomSection { .. } => {}
            wasmparser::Payload::CodeSectionStart { count, range, size } => {}
            wasmparser::Payload::ModuleSectionStart { .. } => {}
            wasmparser::Payload::UnknownSection { .. } => {}
            wasmparser::Payload::ModuleSectionEntry { .. } => {}
        }
    }

    Ok(())
}
