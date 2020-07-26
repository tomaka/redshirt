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

//! Main kernel module.
//!
//! # Usage
//!
//! - Create a type that implements the [`PlatformSpecific`] trait.
//! - From one CPU, create a [`Kernel`] with [`Kernel::init`].
//! - Share the newly-created [`Kernel`] between CPUs, and call [`Kernel::run`] once for each CPU.
//!

use crate::arch::PlatformSpecific;

use alloc::{sync::Arc, vec::Vec};
use core::{
    convert::TryFrom as _,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use futures::prelude::*;
use redshirt_core::{
    build_wasm_module, extrinsics::wasi::WasiExtrinsics, module::ModuleHash, System,
};

/// Main struct of this crate. Runs everything.
pub struct Kernel<TPlat> {
    /// Contains the list of all processes, threads, interfaces, messages, and so on.
    system: System<'static, WasiExtrinsics>,
    /// Has one entry for each CPU. Never resized.
    // TODO: add a way to report these values, see https://github.com/tomaka/redshirt/issues/117
    cpu_busy_counters: Vec<CpuCounter>,
    /// Platform-specific getters. Passed at initialization.
    platform_specific: Pin<Arc<TPlat>>,
}

#[derive(Debug)]
struct CpuCounter {
    /// Total number of nanoseconds spent working since [`Kernel::run`] has been called.
    busy_ticks: atomic::Atomic<u128>,
    /// Total number of nanoseconds spent idle since [`Kernel::run`] has been called.
    idle_ticks: atomic::Atomic<u128>,
}

impl<TPlat> Kernel<TPlat>
where
    TPlat: PlatformSpecific,
{
    /// Initializes a new `Kernel`.
    pub fn init(platform_specific: TPlat) -> Self {
        let platform_specific = Arc::pin(platform_specific);

        // TODO: don't do this on platforms that don't have PCI?
        let pci_devices = unsafe { crate::pci::pci::init_cam_pci() };

        let mut system_builder =
            redshirt_core::system::SystemBuilder::new(WasiExtrinsics::default())
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
                .with_startup_process(build_wasm_module!("../../../modules/log-to-kernel"))
                .with_startup_process(build_wasm_module!("../../../modules/http-server"))
                .with_startup_process(build_wasm_module!("../../../modules/hello-world"))
                .with_startup_process(build_wasm_module!("../../../modules/network-manager"))
                .with_startup_process(build_wasm_module!("../../../modules/e1000"));

        // TODO: remove the cfg guards once rpi-framebuffer is capable of auto-detecting whether
        // it should enable itself
        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        {
            system_builder = system_builder
                .with_startup_process(build_wasm_module!("../../../modules/rpi-framebuffer"))
        }

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

        Kernel {
            system: system_builder.build().expect("failed to start kernel"),
            cpu_busy_counters,
            platform_specific,
        }
    }

    /// Run the kernel. Must be called once per CPU.
    // TODO: check whether cpu_index is correct? (i.e. not the same index passed twice)
    pub async fn run(&self, cpu_index: usize) -> ! {
        assert!(
            u32::try_from(cpu_index).unwrap() < self.platform_specific.as_ref().num_cpus().get()
        );

        // In order for the idle/busy counters to report accurate information, we keep here the
        // last time we have updated one of the counters.
        let mut now = self.platform_specific.as_ref().monotonic_clock();

        loop {
            // Wrap around `self.system.run()` and add time reports to the CPU idle/busy counters.
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
                _ => panic!(),
            }
        }
    }
}
