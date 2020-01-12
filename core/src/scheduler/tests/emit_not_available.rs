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

use crate::InterfaceHash;
use crate::module::Module;
use crate::scheduler::{Core, CoreRunOutcome};

#[test]
fn emit_not_available() {
    /* Original code:
    let interface = redshirt_syscalls_interface::InterfaceHash::from_raw_hash([
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
        0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
    ]);

    unsafe {
        let _ = redshirt_syscalls_interface::MessageBuilder::default()
            .add_data_raw(&[1, 2, 3, 4, 5, 6, 7, 8])
            .emit_without_response(&interface);
    }
    */
    let module = Module::from_wat(r#"
(module
    (type $t0 (func (param i32 i32 i32 i32 i32 i32) (result i32)))
    (type $t1 (func (param i32 i32) (result i32)))
    (type $t2 (func (param i32 i32) (result i64)))
    (type $t3 (func (param i32 i64 i64 i64 i64)))
    (import "redshirt" "emit_message" (func $_ZN27redshirt_syscalls_interface3ffi12emit_message17h508280f1400e36efE (type $t0)))
    (func $main (type $t1) (param $p0 i32) (param $p1 i32) (result i32)
      (local $l0 i32)
      get_global $g0
      i32.const 64
      i32.sub
      tee_local $l0
      set_global $g0
      get_local $l0
      i64.const 3978425819141910832
      i64.store offset=32
      get_local $l0
      i64.const 2820983053732684064
      i64.store offset=24
      get_local $l0
      i64.const 1663540288323457296
      i64.store offset=16
      get_local $l0
      i64.const 506097522914230528
      i64.store offset=8
      get_local $l0
      i32.const 1048576
      i64.extend_u/i32
      i64.const 34359738368
      i64.or
      i64.store offset=41 align=1
      get_local $l0
      i32.const 1
      i32.store8 offset=40
      get_local $l0
      i32.const 8
      i32.add
      get_local $l0
      i32.const 40
      i32.add
      i32.const 1
      i32.or
      i32.const 1
      i32.const 0
      i32.const 1
      get_local $l0
      i32.const 56
      i32.add
      call $_ZN27redshirt_syscalls_interface3ffi12emit_message17h508280f1400e36efE
      drop
      get_local $l0
      i32.const 64
      i32.add
      set_global $g0
      i32.const 0)
    (func $hash_test (type $t2) (param $p0 i32) (param $p1 i32) (result i64)
      (local $l0 i32) (local $l1 i64) (local $l2 i64) (local $l3 i64)
      get_global $g0
      i32.const 32
      i32.sub
      tee_local $l0
      set_global $g0
      get_local $p1
      i64.extend_u/i32
      i64.const 6364136223846793005
      i64.mul
      i64.const 12345
      i64.add
      set_local $l1
      block $B0
        block $B1
          block $B2
            block $B3
              block $B4
                get_local $p1
                i32.const 8
                i32.gt_u
                br_if $B4
                get_local $p1
                i32.const 1
                i32.gt_u
                br_if $B3
                get_local $p1
                br_if $B2
                i64.const 0
                set_local $l2
                br $B1
              end
              block $B5
                get_local $p1
                i32.const 16
                i32.gt_u
                br_if $B5
                get_local $l0
                i32.const 16
                i32.add
                get_local $p0
                i64.load align=1
                get_local $l1
                i64.xor
                i64.const 0
                i64.const 6364136223846793005
                i64.const 0
                call $__multi3
                get_local $l0
                i32.const 24
                i32.add
                i64.load
                get_local $l0
                i64.load offset=16
                i64.add
                get_local $p1
                get_local $p0
                i32.add
                i32.const -8
                i32.add
                i64.load align=1
                i64.xor
                set_local $l2
                br $B0
              end
              get_local $p1
              get_local $p0
              i32.add
              i32.const -8
              i32.add
              i64.load align=1
              set_local $l3
              get_local $l1
              set_local $l2
              loop $L6
                get_local $p0
                i64.load align=1
                get_local $l2
                i64.xor
                i64.const 6364136223846793005
                i64.mul
                i64.const 23
                i64.rotl
                i64.const 6364136223846793005
                i64.mul
                get_local $l1
                i64.xor
                set_local $l1
                get_local $p0
                i32.const 8
                i32.add
                set_local $p0
                get_local $l2
                i64.const 1442695040888963407
                i64.add
                set_local $l2
                get_local $p1
                i32.const -8
                i32.add
                tee_local $p1
                i32.const 8
                i32.gt_u
                br_if $L6
              end
              get_local $l1
              get_local $l3
              i64.xor
              set_local $l2
              br $B0
            end
            block $B7
              get_local $p1
              i32.const 3
              i32.gt_u
              br_if $B7
              get_local $p1
              get_local $p0
              i32.add
              i32.const -2
              i32.add
              i64.load16_u align=1
              i64.const 16
              i64.shl
              get_local $p0
              i64.load16_u align=1
              i64.or
              get_local $l1
              i64.xor
              set_local $l2
              br $B0
            end
            get_local $p1
            get_local $p0
            i32.add
            i32.const -4
            i32.add
            i64.load32_u align=1
            i64.const 32
            i64.shl
            get_local $p0
            i64.load32_u align=1
            i64.or
            get_local $l1
            i64.xor
            set_local $l2
            br $B0
          end
          get_local $p0
          i64.load8_u
          set_local $l2
        end
        get_local $l2
        get_local $l1
        i64.xor
        set_local $l2
      end
      get_local $l0
      get_local $l2
      i64.const 0
      i64.const 6364136223846793005
      i64.const 0
      call $__multi3
      get_local $l0
      i32.const 8
      i32.add
      i64.load
      set_local $l2
      get_local $l0
      i64.load
      set_local $l1
      get_local $l0
      i32.const 32
      i32.add
      set_global $g0
      get_local $l2
      get_local $l1
      i64.add
      i64.const 67
      i64.xor)
    (func $__multi3 (type $t3) (param $p0 i32) (param $p1 i64) (param $p2 i64) (param $p3 i64) (param $p4 i64)
      (local $l0 i64) (local $l1 i64)
      get_local $p0
      get_local $p3
      i64.const 32
      i64.shr_u
      tee_local $l0
      get_local $p1
      i64.const 32
      i64.shr_u
      tee_local $l1
      i64.mul
      get_local $p3
      get_local $p2
      i64.mul
      i64.add
      get_local $p4
      get_local $p1
      i64.mul
      i64.add
      get_local $p3
      i64.const 4294967295
      i64.and
      tee_local $p3
      get_local $p1
      i64.const 4294967295
      i64.and
      tee_local $p1
      i64.mul
      tee_local $p4
      i64.const 32
      i64.shr_u
      get_local $p3
      get_local $l1
      i64.mul
      i64.add
      tee_local $p3
      i64.const 32
      i64.shr_u
      i64.add
      get_local $p3
      i64.const 4294967295
      i64.and
      get_local $l0
      get_local $p1
      i64.mul
      i64.add
      tee_local $p3
      i64.const 32
      i64.shr_u
      i64.add
      i64.store offset=8
      get_local $p0
      get_local $p3
      i64.const 32
      i64.shl
      get_local $p4
      i64.const 4294967295
      i64.and
      i64.or
      i64.store)
    (table $T0 1 1 anyfunc)
    (memory $memory 17)
    (global $g0 (mut i32) (i32.const 1048576))
    (global $__data_end i32 (i32.const 1048584))
    (global $__heap_base i32 (i32.const 1048584))
    (export "memory" (memory 0))
    (export "__data_end" (global 1))
    (export "__heap_base" (global 2))
    (export "main" (func $main))
    (export "hash_test" (func $hash_test))
    (data (i32.const 1048576) "\01\02\03\04\05\06\07\08"))"#).unwrap();

    let mut core = Core::new().build();
    let pid = core.execute(&module).unwrap().pid();
    match core.run() {
        CoreRunOutcome::ThreadWaitUnavailableInterface { thread, interface } => {
            assert_eq!(thread.pid(), pid);
            let expected = InterfaceHash::from_raw_hash([
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
                0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
            ]);
            assert_eq!(interface, expected);
        },
        _ => panic!()
    }
}
