// Copyright (C) 2019  Pierre Krieger
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

#![cfg(test)]

use super::{Core, CoreRunOutcome};
use crate::{
    module::Module,
    signature::{Signature, ValueType},
};
use core::iter;

#[test]
fn basic_module() {
    let module = Module::from_wat(
        r#"(module
        (func $_start (result i32)
            i32.const 5)
        (export "_start" (func $_start)))
    "#,
    )
    .unwrap();

    let mut core = Core::<()>::new().build();
    let expected_pid = core.execute(&module).unwrap().pid();

    match core.run() {
        CoreRunOutcome::ProgramFinished {
            pid,
            outcome: Ok(ret_val),
            ..
        } => {
            assert_eq!(pid, expected_pid);
            assert_eq!(ret_val, Some(wasmi::RuntimeValue::I32(5)));
        }
        _ => panic!(),
    }
}

#[test]
#[ignore] // TODO: test fails
fn trapping_module() {
    let module = Module::from_wat(
        r#"(module
        (func $main (param $p0 i32) (param $p1 i32) (result i32)
            unreachable)
        (export "main" (func $main)))
    "#,
    )
    .unwrap();

    let mut core = Core::<()>::new().build();
    let expected_pid = core.execute(&module).unwrap().pid();

    match core.run() {
        CoreRunOutcome::ProgramFinished {
            pid,
            outcome: Err(_),
            ..
        } => {
            assert_eq!(pid, expected_pid);
        }
        _ => panic!(),
    }
}

#[test]
fn module_wait_extrinsic() {
    let module = Module::from_wat(
        r#"(module
        (import "foo" "test" (func $test (result i32)))
        (func $_start (result i32)
            call $test)
        (export "_start" (func $_start)))
    "#,
    )
    .unwrap();

    let mut core = Core::<u32>::new()
        .with_extrinsic(
            "foo",
            "test",
            Signature::new(iter::empty(), Some(ValueType::I32)),
            639u32,
        )
        .build();

    let expected_pid = core.execute(&module).unwrap().pid();

    let thread_id = match core.run() {
        CoreRunOutcome::ThreadWaitExtrinsic {
            mut thread,
            extrinsic,
            params,
        } => {
            assert_eq!(thread.pid(), expected_pid);
            assert_eq!(extrinsic, 639);
            assert!(params.is_empty());
            thread.tid()
        }
        _ => panic!(),
    };

    core.thread_by_id(thread_id)
        .unwrap()
        .resolve_extrinsic_call(Some(wasmi::RuntimeValue::I32(713)));

    match core.run() {
        CoreRunOutcome::ProgramFinished {
            pid,
            outcome: Ok(ret_val),
            ..
        } => {
            assert_eq!(pid, expected_pid);
            assert_eq!(ret_val, Some(wasmi::RuntimeValue::I32(713)));
        }
        _ => panic!(),
    }
}
