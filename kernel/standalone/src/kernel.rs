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

use crate::arch::PlatformSpecific;

use alloc::{format, string::String, sync::Arc, vec::Vec};
use core::{convert::TryFrom as _, pin::Pin, sync::atomic::Ordering};
use futures::prelude::*;
use hashbrown::HashSet;
use redshirt_core::{build_wasm_module, extrinsics::wasi::WasiExtrinsics, System};
use spinning_top::Spinlock;

/// Main struct of this crate. Runs everything.
pub struct Kernel {
    /// Contains the list of all processes, threads, interfaces, messages, and so on.
    system: System<'static, WasiExtrinsics>,
    /// Has one entry for each CPU. Never resized.
    cpu_busy_counters: Vec<CpuCounter>,
    /// Platform-specific getters. Passed at initialization.
    platform_specific: Pin<Arc<PlatformSpecific>>,
    /// List of CPUs for which [`Kernel::run`] hasn't been called yet. Also makes it possible to
    /// make sure that [`Kernel::run`] isn't called twice with the same CPU index.
    not_started_cpus: Spinlock<HashSet<usize, fnv::FnvBuildHasher>>,
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

        let system_builder = redshirt_core::system::SystemBuilder::new(WasiExtrinsics::default())
            .with_native_program(crate::hardware::HardwareHandler::new(
                platform_specific.clone(),
            ))
            .with_native_program(crate::time::TimeHandler::new(platform_specific.clone()))
            .with_native_program(crate::random::native::RandomNativeProgram::new(
                platform_specific.clone(),
            ))
            .with_native_program(crate::pci::native::PciNativeProgram::new(
                pci_devices,
                platform_specific.clone(),
            ))
            .with_native_program(crate::klog::KernelLogNativeProgram::new(
                platform_specific.clone(),
            ))
            .with_startup_process(build_wasm_module!(
                "../../../modules/p2p-loader",
                "modules-loader"
            ))
            .with_startup_process(build_wasm_module!("../../../modules/compositor"))
            .with_startup_process(build_wasm_module!("../../../modules/pci-printer"))
            // TODO: actually implement system-time and remove this dummy; https://github.com/tomaka/redshirt/issues/542
            .with_startup_process(build_wasm_module!("../../../modules/dummy-system-time"))
            .with_startup_process(build_wasm_module!("../../../modules/log-to-kernel"))
            .with_startup_process(build_wasm_module!("../../../modules/vga-vbe"))
            .with_startup_process(build_wasm_module!(
                "../../../modules/diagnostics-http-server"
            ))
            .with_startup_process(build_wasm_module!("../../../modules/hello-world"))
            .with_startup_process(build_wasm_module!("../../../modules/network-manager"))
            .with_startup_process(build_wasm_module!("../../../modules/e1000"));

        // TODO: remove the cfg guards once rpi-framebuffer is capable of auto-detecting whether
        // it should enable itself
        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        let system_builder = system_builder
            .with_startup_process(build_wasm_module!("../../../modules/rpi-framebuffer"));

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
            platform_specific,
            not_started_cpus,
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

            match fut.await {
                redshirt_core::system::SystemRunOutcome::ProgramFinished { .. } => {}
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
            }
        }
    }
}
