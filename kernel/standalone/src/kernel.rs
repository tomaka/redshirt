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

//! Main kernel module.
//!
//! # Usage
//!
//! - Create a type that implements the [`PlatformSpecific`] trait.
//! - From one CPU, create a [`Kernel`] with [`Kernel::init`].
//! - Share the newly-created [`Kernel`] between CPUs, and call [`Kernel::run`] once for each CPU.
//!

use crate::{
    arch::PlatformSpecific, hardware::HardwareHandler, klog::KernelLogNativeProgram,
    pci::native::PciNativeProgram, random::native::RandomNativeProgram, time::TimeHandler,
};

use alloc::{format, string::String, sync::Arc, vec::Vec};
use core::{convert::TryFrom as _, pin::Pin, sync::atomic::Ordering};
use futures::prelude::*;
use hashbrown::HashSet;
use redshirt_core::{build_wasm_module, extrinsics::wasi::WasiExtrinsics, System};
use spinning_top::Spinlock;

/// Main struct of this crate. Runs everything.
pub struct Kernel {
    /// Contains the list of all processes, threads, interfaces, messages, and so on.
    system: System<WasiExtrinsics>,
    /// Has one entry for each CPU. Never resized.
    cpu_busy_counters: Vec<CpuCounter>,
    /// Platform-specific getters. Passed at initialization.
    platform_specific: Pin<Arc<PlatformSpecific>>,
    /// List of CPUs for which [`Kernel::run`] hasn't been called yet. Also makes it possible to
    /// make sure that [`Kernel::run`] isn't called twice with the same CPU index.
    not_started_cpus: Spinlock<HashSet<usize, fnv::FnvBuildHasher>>,
    time: TimeHandler,
    randomness: RandomNativeProgram,
    hardware: HardwareHandler,
    pci: PciNativeProgram,
    klog: KernelLogNativeProgram,
}

#[derive(Debug)]
struct CpuCounter {
    /// Total number of nanoseconds spent working since [`Kernel::run`] has been called.
    busy_ticks: atomic::Atomic<u128>,
    /// Total number of nanoseconds spent idle since [`Kernel::run`] has been called.
    idle_ticks: atomic::Atomic<u128>,
}

impl Kernel {
    /// Initializes a new `Kernel`.
    pub fn init(platform_specific: Pin<Arc<PlatformSpecific>>) -> Self {
        // TODO: don't do this on platforms that don't have PCI?
        let pci_devices = unsafe { crate::pci::pci::init_cam_pci() };

        let randomness = RandomNativeProgram::new(platform_specific.clone());

        let mut rng_seed = [0; 64];
        randomness.fill_bytes(&mut rng_seed);

        let system_builder =
            redshirt_core::system::SystemBuilder::new(WasiExtrinsics::default(), rng_seed)
                .with_native_interface_handler(redshirt_hardware_interface::ffi::INTERFACE)
                .with_native_interface_handler(redshirt_time_interface::ffi::INTERFACE)
                .with_native_interface_handler(redshirt_random_interface::ffi::INTERFACE)
                .with_native_interface_handler(redshirt_pci_interface::ffi::INTERFACE)
                .with_native_interface_handler(redshirt_kernel_log_interface::ffi::INTERFACE)
                .with_startup_process(build_wasm_module!(
                    "../../../programs/p2p-loader",
                    "programs-loader"
                ))
                .with_startup_process(build_wasm_module!("../../../programs/compositor"))
                .with_startup_process(build_wasm_module!("../../../programs/pci-printer"))
                // TODO: actually implement system-time and remove this dummy; https://github.com/tomaka/redshirt/issues/542
                .with_startup_process(build_wasm_module!("../../../programs/dummy-system-time"))
                .with_startup_process(build_wasm_module!("../../../programs/log-to-kernel"))
                .with_startup_process(build_wasm_module!("../../../programs/vga-vbe"))
                .with_startup_process(build_wasm_module!(
                    "../../../programs/diagnostics-http-server"
                ))
                .with_startup_process(build_wasm_module!("../../../programs/hello-world"))
                .with_startup_process(build_wasm_module!("../../../programs/network-manager"))
                .with_startup_process(build_wasm_module!("../../../programs/e1000"));

        // TODO: remove the cfg guards once rpi-framebuffer is capable of auto-detecting whether
        // it should enable itself
        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        let system_builder = system_builder
            .with_startup_process(build_wasm_module!("../../../programs/rpi-framebuffer"));

        // TODO: temporary; uncomment to test
        /*system_builder = system_builder.with_main_program(
            ModuleHash::from_base58("FWMwRMQCKdWVDdKyx6ogQ8sXuoeDLNzZxniRMyD5S71").unwrap(),
        );*/

        let cpu_busy_counters = (0..platform_specific.as_ref().num_cpus().get())
            .map(|_| CpuCounter {
                busy_ticks: atomic::Atomic::new(0),
                idle_ticks: atomic::Atomic::new(0),
            })
            .collect();

        let not_started_cpus = Spinlock::new(
            (0..usize::try_from(platform_specific.as_ref().num_cpus().get()).unwrap()).collect(),
        );

        Kernel {
            system: system_builder.build().expect("failed to start kernel"),
            cpu_busy_counters,
            platform_specific: platform_specific.clone(),
            not_started_cpus,
            time: TimeHandler::new(platform_specific.clone()),
            randomness,
            hardware: HardwareHandler::new(platform_specific.clone()),
            pci: PciNativeProgram::new(pci_devices, platform_specific.clone()),
            klog: KernelLogNativeProgram::new(platform_specific.clone()),
        }
    }

    /// Run the kernel. Must be called once per CPU.
    pub async fn run(&self, cpu_index: usize) -> ! {
        // Check that the `cpu_index` is correct.
        {
            let mut not_started_cpus = self.not_started_cpus.lock();
            let _was_in = not_started_cpus.remove(&cpu_index);
            assert!(_was_in);
            if not_started_cpus.is_empty() {
                // Un-allocate memory.
                not_started_cpus.shrink_to_fit();
            }
        }

        // In order for the idle/busy counters to report accurate information, we keep here the
        // last time we have updated one of the counters.
        let mut now = self.platform_specific.as_ref().monotonic_clock();

        loop {
            // Wrap around `self.system.run()` and add time reports to the CPU idle/busy counters.
            // TODO: because of this implementation, the idle counter is only updated when the
            // CPU has some work to do; in other words, if a CPU is asleep for a long time then its
            // counters will not be updated
            let inner = self.system.run();
            futures::pin_mut!(inner);
            let fut = future::poll_fn(|cx| {
                let new_now = self.platform_specific.as_ref().monotonic_clock();
                let elapsed_idle = new_now.checked_sub(now).unwrap();
                now = new_now;
                self.cpu_busy_counters[cpu_index]
                    .idle_ticks
                    .fetch_add(elapsed_idle, Ordering::Relaxed);

                let outcome = Future::poll(inner.as_mut(), cx);

                let new_now = self.platform_specific.as_ref().monotonic_clock();
                let elapsed_budy = new_now.checked_sub(now).unwrap();
                now = new_now;
                self.cpu_busy_counters[cpu_index]
                    .busy_ticks
                    .fetch_add(elapsed_budy, Ordering::Relaxed);

                outcome
            });

            // TODO: clean up this general function body

            let next_time_response = self.time.next_response();
            let next_pci_response = self.pci.next_response();
            futures::pin_mut!(next_time_response, next_pci_response);

            let interface_handlers =
                future::select(next_time_response, next_pci_response).map(|e| e.factor_first().0);

            let core_event = match future::select(fut, interface_handlers).await {
                future::Either::Left((event, _)) => event,
                future::Either::Right(((message_id, response), _)) => {
                    self.system.answer_message(message_id, Ok(response));
                    continue;
                }
            };

            match core_event {
                redshirt_core::system::SystemRunOutcome::ProgramFinished { pid, .. } => {
                    self.hardware.process_destroyed(pid);
                }
                redshirt_core::system::SystemRunOutcome::KernelDebugMetricsRequest(report) => {
                    let mut out = String::new();

                    // cpu_idle_seconds_total
                    out.push_str("# HELP redshirt_cpu_idle_seconds_total Total number of seconds during which each CPU has been idle.\n");
                    out.push_str("# TYPE redshirt_cpu_idle_seconds_total counter\n");
                    for (cpu_n, cpu) in self.cpu_busy_counters.iter().enumerate() {
                        let as_secs =
                            cpu.idle_ticks.load(Ordering::Relaxed) as f64 / 1_000_000_000.0;
                        out.push_str(&format!(
                            "redshirt_cpu_idle_seconds_total{{cpu=\"{}\"}} {}\n",
                            cpu_n, as_secs
                        ));
                    }
                    out.push_str("\n");

                    // cpu_busy_seconds_total
                    out.push_str("# HELP redshirt_cpu_busy_seconds_total Total number of seconds during which each CPU has been busy.\n");
                    out.push_str("# TYPE redshirt_cpu_busy_seconds_total counter\n");
                    for (cpu_n, cpu) in self.cpu_busy_counters.iter().enumerate() {
                        let as_secs =
                            cpu.busy_ticks.load(Ordering::Relaxed) as f64 / 1_000_000_000.0;
                        out.push_str(&format!(
                            "redshirt_cpu_busy_seconds_total{{cpu=\"{}\"}} {}\n",
                            cpu_n, as_secs
                        ));
                    }
                    out.push_str("\n");

                    // monotonic_clock
                    out.push_str("# HELP redshirt_monotonic_clock Value of the monotonic clock.\n");
                    out.push_str("# TYPE redshirt_monotonic_clock counter\n");
                    out.push_str(&format!("redshirt_monotonic_clock {}\n", now));
                    out.push_str("\n");

                    // num_cpus
                    out.push_str("# HELP redshirt_num_cpus Number of CPUs on the machine.\n");
                    out.push_str("# TYPE redshirt_num_cpus counter\n");
                    out.push_str(&format!(
                        "redshirt_num_cpus {}\n",
                        self.cpu_busy_counters.len()
                    ));
                    out.push_str("\n");

                    report.respond(&out);
                }

                // Time handling.
                redshirt_core::system::SystemRunOutcome::NativeInterfaceMessage {
                    interface,
                    message_id: Some(message_id),
                    message,
                    ..
                } if interface == redshirt_time_interface::ffi::INTERFACE => {
                    if let Some(response) = self.time.interface_message(message_id, message) {
                        self.system.answer_message(message_id, response);
                    }
                }
                redshirt_core::system::SystemRunOutcome::NativeInterfaceMessage {
                    interface,
                    message_id: None,
                    ..
                } if interface == redshirt_time_interface::ffi::INTERFACE => {}

                // Randomness queries handling.
                redshirt_core::system::SystemRunOutcome::NativeInterfaceMessage {
                    interface,
                    message_id: Some(message_id),
                    message,
                    ..
                } if interface == redshirt_random_interface::ffi::INTERFACE => {
                    let response = self.randomness.interface_message(message);
                    self.system.answer_message(message_id, response);
                }
                redshirt_core::system::SystemRunOutcome::NativeInterfaceMessage {
                    interface,
                    message_id: None,
                    ..
                } if interface == redshirt_random_interface::ffi::INTERFACE => {}

                // Hardware handling.
                redshirt_core::system::SystemRunOutcome::NativeInterfaceMessage {
                    interface,
                    message_id: Some(message_id),
                    message,
                    emitter_pid,
                    ..
                } if interface == redshirt_hardware_interface::ffi::INTERFACE => {
                    if let Some(response) = self.hardware.interface_message(emitter_pid, message) {
                        self.system.answer_message(message_id, response);
                    }
                }
                redshirt_core::system::SystemRunOutcome::NativeInterfaceMessage {
                    interface,
                    message_id: None,
                    emitter_pid,
                    message,
                    ..
                } if interface == redshirt_hardware_interface::ffi::INTERFACE => {
                    self.hardware.interface_message(emitter_pid, message);
                }

                // PCI handling.
                redshirt_core::system::SystemRunOutcome::NativeInterfaceMessage {
                    interface,
                    message_id: Some(message_id),
                    message,
                    emitter_pid,
                    ..
                } if interface == redshirt_pci_interface::ffi::INTERFACE => {
                    if let Some(response) =
                        self.pci
                            .interface_message(Some(message_id), emitter_pid, message)
                    {
                        self.system.answer_message(message_id, response);
                    }
                }
                redshirt_core::system::SystemRunOutcome::NativeInterfaceMessage {
                    interface,
                    message_id: None,
                    emitter_pid,
                    message,
                } if interface == redshirt_pci_interface::ffi::INTERFACE => {
                    self.pci.interface_message(None, emitter_pid, message);
                }

                // Kernel logs handling.
                redshirt_core::system::SystemRunOutcome::NativeInterfaceMessage {
                    interface,
                    message,
                    ..
                } if interface == redshirt_kernel_log_interface::ffi::INTERFACE => {
                    self.klog.interface_message(&message);
                }

                redshirt_core::system::SystemRunOutcome::NativeInterfaceMessage { .. } => {
                    unreachable!()
                }
            }
        }
    }
}
