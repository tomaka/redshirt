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

use crate::extrinsics::NoExtrinsics;
use crate::scheduler::{CoreBuilder, CoreRunOutcome};
use crate::InterfaceHash;
use futures::prelude::*;

#[test]
fn emit_not_available() {
    /* Original code:

    let interface = redshirt_syscalls::InterfaceHash::from_raw_hash([
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
        0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
    ]);

    unsafe {
        let _ = redshirt_syscalls::MessageBuilder::default()
            .add_data_raw(&[1, 2, 3, 4, 5, 6, 7, 8])
            .emit_without_response(&interface);
    }

    */
    let module = from_wat!(
        local,
        r#"
(module
    (type $t0 (func (param i32 i32 i32 i64 i32) (result i32)))
    (import "redshirt" "emit_message" (func $_ZN27redshirt_syscalls3ffi12emit_message17h508280f1400e36efE (type $t0)))
    (func $_start (result i32)
        (local $l0 i32)
        global.get $g0
        i32.const 64
        i32.sub
        local.tee $l0
        global.set $g0
        local.get $l0
        i64.const 3978425819141910832
        i64.store offset=32
        local.get $l0
        i64.const 2820983053732684064
        i64.store offset=24
        local.get $l0
        i64.const 1663540288323457296
        i64.store offset=16
        local.get $l0
        i64.const 506097522914230528
        i64.store offset=8
        local.get $l0
        i32.const 1048576
        i64.extend_i32_s
        i64.const 34359738368
        i64.or
        i64.store offset=41 align=1
        local.get $l0
        i32.const 1
        i32.store8 offset=40
        local.get $l0
        i32.const 8
        i32.add
        local.get $l0
        i32.const 40
        i32.add
        i32.const 1
        i32.or
        i32.const 1
        i64.const 2
        local.get $l0
        i32.const 56
        i32.add
        call $_ZN27redshirt_syscalls3ffi12emit_message17h508280f1400e36efE
        drop
        local.get $l0
        i32.const 64
        i32.add
        global.set $g0
        i32.const 0)
    (table $T0 1 1 funcref)
    (memory $memory 17)
    (global $g0 (mut i32) (i32.const 1048576))
    (export "memory" (memory 0))
    (export "_start" (func $_start))
    (data (i32.const 1048576) "\01\02\03\04\05\06\07\08"))"#
    );

    let core = CoreBuilder::<NoExtrinsics>::with_seed([0; 64]).build();
    core.execute(&module).unwrap();

    match core.run().now_or_never().unwrap().or_run() {
        Some(CoreRunOutcome::InterfaceMessage { interface, .. }) => {
            assert_eq!(
                interface,
                InterfaceHash::from_raw_hash([
                    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14,
                    0x15, 0x16, 0x17, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x30, 0x31,
                    0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
                ])
            );
        }
        _ => panic!(),
    }

    assert!(core.run().now_or_never().is_none());
}
