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

impl Kernel {
    /// Initializes a new `Kernel`.
    pub fn init(_cfg: KernelConfig) -> Self {
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

        let hardware = crate::hardware::HardwareHandler::new();

        let mut system =
            redshirt_wasi_hosted::register_extrinsics(redshirt_core::system::SystemBuilder::new())
                .with_interface_handler(redshirt_hardware_interface::ffi::INTERFACE)
                .with_startup_process({
                    // TODO: use a better system than cfgs
                    #[cfg(target_arch = "x86_64")]
                    mod foo { wasi_module_builder::build!("../../../modules/x86-stdout"); }
                    #[cfg(target_arch = "arm")]
                    mod foo { wasi_module_builder::build!("../../../modules/arm-stdout"); }
                    redshirt_core::module::Module::from_bytes(&foo::MODULE_BYTES[..]).unwrap()
                })
                .with_startup_process({
                    mod foo { wasi_module_builder::build!("../../../modules/hello-world"); }
                    redshirt_core::module::Module::from_bytes(&foo::MODULE_BYTES[..]).unwrap()
                })
                .with_main_program([0; 32]) // TODO: just a test
                .build();

        let mut wasi = redshirt_wasi_hosted::WasiStateMachine::new();

        loop {
            match system.run() {
                redshirt_core::system::SystemRunOutcome::Idle => {
                    // TODO: If we don't support any interface or extrinsic, then `Idle` shouldn't
                    // happen. In a normal situation, this is when we would check the status of the
                    // "externalities", such as the timer.
                    //panic!("idle");
                    crate::arch::halt();
                }
                redshirt_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                    pid,
                    thread_id,
                    extrinsic,
                    params,
                } => {
                    let out =
                        wasi.handle_extrinsic_call(&mut system, extrinsic, pid, thread_id, params);
                    if let redshirt_wasi_hosted::HandleOut::EmitMessage {
                        id,
                        interface,
                        message,
                    } = out
                    {
                        if interface == redshirt_stdout_interface::ffi::INTERFACE {
                            let msg =
                                redshirt_stdout_interface::ffi::StdoutMessage::decode_all(&message);
                            system.emit_interface_message_no_answer(interface, msg.unwrap());
                        } else {
                            panic!()
                        }
                    }
                }
                redshirt_core::system::SystemRunOutcome::ProgramFinished { pid, outcome } => {
                    hardware.process_stopped(pid);
                    //console.write(&format!("Program finished {:?} => {:?}\n", pid, outcome));
                }
                redshirt_core::system::SystemRunOutcome::InterfaceMessage {
                    pid,
                    interface,
                    message,
                    message_id,
                } if interface == redshirt_hardware_interface::ffi::INTERFACE => {
                    if let Some(answer) = hardware.hardware_message(pid, message_id, &message) {
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
