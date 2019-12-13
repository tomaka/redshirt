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

//! Main kernel module.
//!
//! # Usage
//!
//! - Create a [`KernelConfig`] struct indicating the configuration.
//! - From one CPU, create a [`Kernel`] with [`Kernel::init`].
//! - Share the newly-created [`Kernel`] between CPUs, and call [`Kernel::run`] once for each CPU.
//!

use alloc::format;
use core::sync::atomic::{AtomicBool, Ordering};
use parity_scale_codec::DecodeAll;

/// Main struct of this crate. Runs everything.
pub struct Kernel {
    /// If true, the kernel has started running from a different thread already.
    running: AtomicBool,
}

/// Configuration for creating a [`Kernel`].
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct KernelConfig {
    /// Number of times the [`Kernel::run`] function might be called.
    pub num_cpus: u32,
}

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

#[alloc_error_handler]
fn alloc_error_handler(_: core::alloc::Layout) -> ! {
    panic!()
}

impl Kernel {
    /// Initializes a new `Kernel`.
    pub fn init(_cfg: KernelConfig) -> Self {
        // TODO: initialize allocator only once?
        unsafe {
            // TODO: don't have the HEAP here, but adjust it to the available RAM
            static mut HEAP: [u8; 0x10000000] = [0; 0x10000000];
            ALLOCATOR
                .lock()
                .init(HEAP.as_mut_ptr() as usize, HEAP.len());
        }

        Kernel {
            running: AtomicBool::new(false),
        }
    }

    /// Run the kernel. Must be called once per CPU.
    pub fn run(&self) -> ! {
        // We only want a single CPU to run for now.
        if self.running.swap(true, Ordering::SeqCst) {
            crate::arch::halt();
        }

        let hardware = nametbd_hardware::HardwareHandler::new();

        let hello_module = nametbd_core::module::Module::from_bytes(
            &include_bytes!("../../../modules/target/wasm32-wasi/release/hello-world.wasm")[..],
        )
        .unwrap();

        let stdout_module = nametbd_core::module::Module::from_bytes(
            &include_bytes!("../../../modules/target/wasm32-wasi/release/x86-stdout.wasm")[..],
        )
        .unwrap();

        let pci_module = nametbd_core::module::Module::from_bytes(
            &include_bytes!("../../../modules/target/wasm32-wasi/release/x86-pci.wasm")[..],
        )
        .unwrap();

        let mut system =
            nametbd_wasi_hosted::register_extrinsics(nametbd_core::system::SystemBuilder::new())
                .with_interface_handler(nametbd_hardware_interface::ffi::INTERFACE)
                .with_startup_process(stdout_module)
                .with_startup_process(hello_module)
                .with_startup_process(pci_module)
                .with_main_program([0; 32]) // TODO: just a test
                .build();

        let mut wasi = nametbd_wasi_hosted::WasiStateMachine::new();

        loop {
            match system.run() {
                nametbd_core::system::SystemRunOutcome::Idle => {
                    // TODO: If we don't support any interface or extrinsic, then `Idle` shouldn't
                    // happen. In a normal situation, this is when we would check the status of the
                    // "externalities", such as the timer.
                    //panic!("idle");
                    crate::arch::halt();
                }
                nametbd_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                    pid,
                    thread_id,
                    extrinsic,
                    params,
                } => {
                    let out =
                        wasi.handle_extrinsic_call(&mut system, extrinsic, pid, thread_id, params);
                    if let nametbd_wasi_hosted::HandleOut::EmitMessage {
                        id,
                        interface,
                        message,
                    } = out
                    {
                        /*if interface == nametbd_stdout_interface::ffi::INTERFACE {
                            let msg =
                                nametbd_stdout_interface::ffi::StdoutMessage::decode_all(&message);
                            let nametbd_stdout_interface::ffi::StdoutMessage::Message(msg) =
                                msg.unwrap();
                            console.write(&msg);
                        } else {*/
                        panic!()
                        //}
                    }
                }
                nametbd_core::system::SystemRunOutcome::ProgramFinished { pid, outcome } => {
                    //console.write(&format!("Program finished {:?} => {:?}\n", pid, outcome));
                }
                nametbd_core::system::SystemRunOutcome::InterfaceMessage {
                    interface,
                    message,
                    message_id,
                } if interface == nametbd_hardware_interface::ffi::INTERFACE => {
                    if let Some(answer) = hardware.hardware_message(message_id, &message) {
                        let answer = match &answer {
                            Ok(v) => Ok(&v[..]),
                            Err(()) => Err(()),
                        };
                        system.answer_message(message_id.unwrap(), answer);
                    }
                }
                _ => panic!(),
            }
        }
    }
}
