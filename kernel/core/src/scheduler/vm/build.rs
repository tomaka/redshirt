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
// TODO: also see https://github.com/bytecodealliance/wasmtime/blob/main/crates/cranelift/src/compiler.rs

use alloc::vec::Vec;
use core::str::FromStr as _;

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

fn test() {
    let isa = {
        // TODO: should detect the host features when building the isa, like in https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/native/src/lib.rs
        let builder = cranelift_codegen::isa::lookup(target_lexicon::triple!("x86_64")).unwrap(); // TODO: don't unwrap
        let flags = cranelift_codegen::settings::Flags::new(cranelift_codegen::settings::builder());
        builder.finish(flags)
    };

    let mut context = cranelift_codegen::Context::new();
    let mut code_out = Vec::new();
    context.compile_and_emit(&*isa, &mut code_out).unwrap(); // TODO: don't unwrap
}
