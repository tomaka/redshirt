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
use core::{pin::Pin, sync::atomic::Ordering};
use futures::prelude::*;
use redshirt_core::{
    extrinsics::wasi::WasiExtrinsics,
    system::{KernelDebugMetricsRequest, SystemRunOutcome},
    System,
};

/// Main struct of this crate. Runs everything.
pub struct Kernel {
    /// Contains the list of all processes, threads, interfaces, messages, and so on.
    system: System<WasiExtrinsics>,
    /// Has one entry for each CPU. Never resized.
    cpu_counters: Vec<CpuCounter>,
    /// Platform-specific getters. Passed at initialization.
    platform_specific: Pin<Arc<PlatformSpecific>>,
    time: TimeHandler,
    randomness: RandomNativeProgram,
    hardware: HardwareHandler,
    pci: PciNativeProgram,
    klog: KernelLogNativeProgram,
}

#[derive(Debug)]
struct CpuCounter {
    /// True if the CPU has been started at all.
    started: atomic::Atomic<bool>,
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

        let system_builder = redshirt_core::system::SystemBuilder::<WasiExtrinsics>::new(rng_seed)
            .with_native_interface_handler(redshirt_hardware_interface::ffi::INTERFACE)
            .with_native_interface_handler(redshirt_time_interface::ffi::INTERFACE)
            .with_native_interface_handler(redshirt_random_interface::ffi::INTERFACE)
            .with_native_interface_handler(redshirt_pci_interface::ffi::INTERFACE)
            .with_native_interface_handler(redshirt_kernel_log_interface::ffi::INTERFACE)
            .with_startup_process(From::from(include_bytes!(env!(
                "CARGO_BIN_FILE_COMPOSITOR"
            ))))
            .with_startup_process(From::from(include_bytes!(env!(
                "CARGO_BIN_FILE_PCI_PRINTER"
            ))))
            .with_startup_process(From::from(include_bytes!(env!(
                "CARGO_BIN_FILE_DUMMY_SYSTEM_TIME"
            ))))
            .with_startup_process(From::from(include_bytes!(env!(
                "CARGO_BIN_FILE_LOG_TO_KERNEL"
            ))))
            .with_startup_process(From::from(include_bytes!(env!("CARGO_BIN_FILE_VGA_VBE"))))
            .with_startup_process(From::from(include_bytes!(env!(
                "CARGO_BIN_FILE_DIAGNOSTICS_HTTP_SERVER"
            ))))
            .with_startup_process(From::from(include_bytes!(env!(
                "CARGO_BIN_FILE_HELLO_WORLD"
            ))))
            .with_startup_process(From::from(include_bytes!(env!(
                "CARGO_BIN_FILE_NETWORK_MANAGER"
            ))))
            .with_startup_process(From::from(include_bytes!(env!("CARGO_BIN_FILE_E1000"))));

        // TODO: remove the cfg guards once rpi-framebuffer is capable of auto-detecting whether
        // it should enable itself
        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        let system_builder = system_builder.with_startup_process(From::from(include_bytes!(env!(
            "CARGO_BIN_FILE_RPI_FRAMEBUFFER"
        ))));

        // TODO: temporary; uncomment to test
        /*system_builder = system_builder.with_main_program(
            ModuleHash::from_base58("FWMwRMQCKdWVDdKyx6ogQ8sXuoeDLNzZxniRMyD5S71").unwrap(),
        );*/

        let cpu_counters = (0..platform_specific.as_ref().num_cpus().get())
            .map(|_| CpuCounter {
                started: atomic::Atomic::new(false),
                busy_ticks: atomic::Atomic::new(0),
                idle_ticks: atomic::Atomic::new(0),
            })
            .collect();

        Kernel {
            system: system_builder.build().expect("failed to start kernel"),
            cpu_counters,
            platform_specific: platform_specific.clone(),
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
            let was_started = self.cpu_counters[cpu_index]
                .started
                .swap(true, Ordering::Relaxed);
            assert!(!was_started);
        }

        // In order for the idle/busy counters to report accurate information, we keep here the
        // last time we have updated one of the counters.
        let mut now = self.platform_specific.as_ref().monotonic_clock();

        loop {
            // Prepare `interface_handlers`, the future that polls the external interface handlers
            // for new message answers.
            let next_time_response = self.time.next_response();
            let next_pci_response = self.pci.next_response();
            futures::pin_mut!(next_time_response, next_pci_response);
            let mut interface_handlers =
                future::select(next_time_response, next_pci_response).map(|e| e.factor_first().0);

            // Poll the interface handlers first in order to guarantee that messages are answered
            // in between two program executions in the core.
            if let Some((message_id, response)) = (&mut interface_handlers).now_or_never() {
                self.system.answer_message(message_id, Ok(response));
                continue;
            }

            // Ask the core for the next event, or the next process execution to perform.
            let core_work = self.system.run();
            futures::pin_mut!(core_work);
            let core_event = match future::select(interface_handlers, core_work).await {
                future::Either::Right((event, _)) => event,
                future::Either::Left(((message_id, response), _)) => {
                    self.system.answer_message(message_id, Ok(response));
                    continue;
                }
            };

            // Grabbing the value of the monotonic clock is a "semi-expensive" operation. It is
            // grabbed here because it is potentially needed below.
            let new_now = self.platform_specific.as_ref().monotonic_clock();

            let ready_to_run = match core_event {
                redshirt_core::ExecuteOut::Direct(ev) => {
                    self.handle_event(ev, new_now);
                    continue;
                }
                redshirt_core::ExecuteOut::ReadyToRun(ready_to_run) => ready_to_run,
            };

            // The code below has the objective of calling `ready_to_run.run()`. We need to
            // update the CPU counters in order to report the CPU as busy during the execution.

            // TODO: because of the way it is implemented, the idle counter is only updated when the
            // CPU has some work to do; in other words, if a CPU is asleep for a long time then its
            // counters will not be updated
            let elapsed_idle = new_now.checked_sub(now).unwrap();
            now = new_now;
            self.cpu_counters[cpu_index]
                .idle_ticks
                .fetch_add(elapsed_idle, Ordering::Relaxed);

            let run_outcome = ready_to_run.run();

            let new_now = self.platform_specific.as_ref().monotonic_clock();
            let elapsed_budy = new_now.checked_sub(now).unwrap();
            now = new_now;
            self.cpu_counters[cpu_index]
                .busy_ticks
                .fetch_add(elapsed_budy, Ordering::Relaxed);

            if let Some(event) = run_outcome {
                self.handle_event(event, now);
            }
        }
    }

    fn handle_event(
        &self,
        core_event: SystemRunOutcome<WasiExtrinsics>,
        monotonic_clock_value: u128,
    ) {
        match core_event {
            SystemRunOutcome::ProgramFinished { pid, .. } => {
                self.hardware.process_destroyed(pid);
            }
            SystemRunOutcome::KernelDebugMetricsRequest(report) => {
                self.report_kernel_metrics(report, monotonic_clock_value);
            }

            // Time handling.
            SystemRunOutcome::NativeInterfaceMessage {
                interface,
                message_id: Some(message_id),
                message,
                ..
            } if interface == redshirt_time_interface::ffi::INTERFACE => {
                if let Some(response) = self.time.interface_message(message_id, message) {
                    self.system.answer_message(message_id, response);
                }
            }
            SystemRunOutcome::NativeInterfaceMessage {
                interface,
                message_id: None,
                ..
            } if interface == redshirt_time_interface::ffi::INTERFACE => {}

            // Randomness queries handling.
            SystemRunOutcome::NativeInterfaceMessage {
                interface,
                message_id: Some(message_id),
                message,
                ..
            } if interface == redshirt_random_interface::ffi::INTERFACE => {
                let response = self.randomness.interface_message(message);
                self.system.answer_message(message_id, response);
            }
            SystemRunOutcome::NativeInterfaceMessage {
                interface,
                message_id: None,
                ..
            } if interface == redshirt_random_interface::ffi::INTERFACE => {}

            // Hardware handling.
            SystemRunOutcome::NativeInterfaceMessage {
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
            SystemRunOutcome::NativeInterfaceMessage {
                interface,
                message_id: None,
                emitter_pid,
                message,
                ..
            } if interface == redshirt_hardware_interface::ffi::INTERFACE => {
                self.hardware.interface_message(emitter_pid, message);
            }

            // PCI handling.
            SystemRunOutcome::NativeInterfaceMessage {
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
            SystemRunOutcome::NativeInterfaceMessage {
                interface,
                message_id: None,
                emitter_pid,
                message,
            } if interface == redshirt_pci_interface::ffi::INTERFACE => {
                self.pci.interface_message(None, emitter_pid, message);
            }

            // Kernel logs handling.
            SystemRunOutcome::NativeInterfaceMessage {
                interface, message, ..
            } if interface == redshirt_kernel_log_interface::ffi::INTERFACE => {
                self.klog.interface_message(message);
            }

            SystemRunOutcome::NativeInterfaceMessage { .. } => {
                unreachable!()
            }
        }
    }

    fn report_kernel_metrics(
        &self,
        report: KernelDebugMetricsRequest<WasiExtrinsics>,
        monotonic_clock_value: u128,
    ) {
        let mut out = String::new();

        // cpu_idle_seconds_total
        out.push_str("# HELP redshirt_cpu_idle_seconds_total Total number of seconds during which each CPU has been idle.\n");
        out.push_str("# TYPE redshirt_cpu_idle_seconds_total counter\n");
        for (cpu_n, cpu) in self.cpu_counters.iter().enumerate() {
            let as_secs = cpu.idle_ticks.load(Ordering::Relaxed) as f64 / 1_000_000_000.0;
            out.push_str(&format!(
                "redshirt_cpu_idle_seconds_total{{cpu=\"{}\"}} {}\n",
                cpu_n, as_secs
            ));
        }
        out.push_str("\n");

        // cpu_busy_seconds_total
        out.push_str("# HELP redshirt_cpu_busy_seconds_total Total number of seconds during which each CPU has been busy.\n");
        out.push_str("# TYPE redshirt_cpu_busy_seconds_total counter\n");
        for (cpu_n, cpu) in self.cpu_counters.iter().enumerate() {
            let as_secs = cpu.busy_ticks.load(Ordering::Relaxed) as f64 / 1_000_000_000.0;
            out.push_str(&format!(
                "redshirt_cpu_busy_seconds_total{{cpu=\"{}\"}} {}\n",
                cpu_n, as_secs
            ));
        }
        out.push_str("\n");

        // monotonic_clock
        out.push_str("# HELP redshirt_monotonic_clock Value of the monotonic clock.\n");
        out.push_str("# TYPE redshirt_monotonic_clock counter\n");
        out.push_str(&format!(
            "redshirt_monotonic_clock {}\n",
            monotonic_clock_value
        ));
        out.push_str("\n");

        // started_cpus
        out.push_str("# HELP redshirt_started_cpus Number of CPUs started on the machine.\n");
        out.push_str("# TYPE redshirt_started_cpus counter\n");
        out.push_str(&format!(
            "redshirt_started_cpus {}\n",
            self.cpu_counters
                .iter()
                .filter(|c| c.started.load(Ordering::Relaxed))
                .count()
        ));
        out.push_str("\n");

        report.respond(&out);
    }
}
