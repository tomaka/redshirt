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

use alloc::sync::Arc;
use core::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
};
use redshirt_core::{build_wasm_module, System};

/// Main struct of this crate. Runs everything.
pub struct Kernel<TPlat> {
    system: System<'static>,
    /// If true, the kernel has started running from a different thread already.
    running: AtomicBool,
    /// Phantom data so that we can keep the platform specific generic parameter.
    marker: PhantomData<TPlat>,
}

impl<TPlat> Kernel<TPlat>
where
    TPlat: PlatformSpecific,
{
    /// Initializes a new `Kernel`.
    pub fn init(platform_specific: TPlat) -> Self {
        let platform_specific = Arc::pin(platform_specific);
        let pci_devices = unsafe { crate::pci::pci::init_cam_pci() };

        let mut system_builder = redshirt_core::system::SystemBuilder::new()
            .with_native_program(crate::hardware::HardwareHandler::new(
                platform_specific.clone(),
            ))
            .with_native_program(crate::time::TimeHandler::new(platform_specific.clone()))
            .with_native_program(crate::random::native::RandomNativeProgram::new(
                platform_specific.clone(),
            ))
            .with_native_program(crate::pci::native::PciNativeProgram::new(pci_devices))
            .with_native_program(crate::klog::KernelLogNativeProgram::new(
                platform_specific.clone(),
            ))
            .with_startup_process(build_wasm_module!(
                "../../../modules/p2p-loader",
                "passive-node"
            ))
            .with_startup_process(build_wasm_module!("../../../modules/log-to-kernel"))
            .with_startup_process(build_wasm_module!("../../../modules/hello-world"));

        // TODO: use a better system than cfgs
        #[cfg(target_arch = "x86_64")]
        {
            system_builder =
                system_builder.with_startup_process(build_wasm_module!("../../../modules/ne2000"))
        }
        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        {
            system_builder = system_builder
                .with_startup_process(build_wasm_module!("../../../modules/rpi-framebuffer"))
        }

        Kernel {
            system: system_builder.build().expect("failed to start kernel"),
            running: AtomicBool::new(false),
            marker: PhantomData,
        }
    }

    /// Run the kernel. Must be called once per CPU.
    pub async fn run(&self) -> ! {
        // TODO: we only run on a single CPU for now, to be cautious
        if self.running.swap(true, Ordering::SeqCst) {
            loop {
                futures::future::poll_fn(|_| core::task::Poll::Pending).await
            }
        }

        loop {
            match self.system.run().await {
                redshirt_core::system::SystemRunOutcome::ProgramFinished { .. } => {}
                _ => panic!(),
            }
        }
    }
}
