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

use crate::module::Module;
use crate::scheduler::{Core, CoreRunOutcome};
use crate::{EncodedMessage, InterfaceHash};

use alloc::vec;
use futures::prelude::*;

#[test]
fn wasm_recv_interface_msg() {
    /* Original code:

    extern crate alloc;
    use alloc::vec;
    use futures::prelude::*;

    #[start]
    fn main(_: isize, _: *const *const u8) -> isize {
        redshirt_syscalls::block_on(async_main());
        0
    }

    fn async_main() -> impl Future<Output = ()> {
        let interface = redshirt_syscalls::InterfaceHash::from_raw_hash([
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
        ]);

        redshirt_syscalls::next_interface_message()
            .then(move |msg| {
                let msg = match msg {
                    redshirt_syscalls::DecodedInterfaceOrDestroyed::Interface(m) => m,
                    redshirt_syscalls::DecodedInterfaceOrDestroyed::ProcessDestroyed(_) => panic!(),
                };
                assert_eq!(msg.interface, interface);
                assert_eq!(msg.actual_data, redshirt_syscalls::EncodedMessage(vec![1, 2, 3, 4, 5, 6, 7, 8]));
                future::ready(())
            })
    }

    */
    let module = Module::from_bytes(&include_bytes!("./wasm_recv_interface_msg.wasm")[..]).unwrap();

    let interface = InterfaceHash::from_raw_hash([
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16,
        0x17, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35,
        0x36, 0x37,
    ]);

    let mut builder = Core::new();
    let reserved_pid = builder.reserve_pid();
    let core = builder.build();
    let wasm_proc_pid = core.execute(&module).unwrap().pid();
    core.set_interface_handler(interface.clone(), wasm_proc_pid)
        .unwrap();

    core.emit_interface_message_no_answer(
        reserved_pid,
        interface,
        EncodedMessage(vec![1, 2, 3, 4, 5, 6, 7, 8]),
    );

    match core.run().now_or_never() {
        Some(CoreRunOutcome::ProgramFinished {
            pid: finished_pid,
            outcome,
            ..
        }) => {
            assert_eq!(finished_pid, wasm_proc_pid);
            assert!(outcome.is_ok());
        }
        _ => panic!(),
    }
}
