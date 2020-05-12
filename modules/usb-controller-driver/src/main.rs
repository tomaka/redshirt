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

//! UHCI, OHCI, EHCI and xHCI driver.
//!
//! This program detects PCI devices that correspond to USB host controllers, and implements the
//! USB interface.
// TODO: only OHCI is implemented lol

use core::{alloc::Layout, convert::TryFrom as _};
use core::{
    num::{NonZeroU32, NonZeroU64, NonZeroU8},
    time::Duration,
};
use futures::prelude::*;
use parity_scale_codec::DecodeAll;

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    for device in redshirt_pci_interface::get_pci_devices().await {
        match (device.class_code, device.subclass, device.prog_if) {
            (0xc, 0x3, 0x0) => {
                // UHCI
                unimplemented!() // TODO:
            }
            (0xc, 0x3, 0x10) => {
                // OHCI
                // TODO: should probably write to LATENCY_TIMER in the PCI config space, as the specs mention
                let addr = match device.base_address_registers[0] {
                    redshirt_pci_interface::PciBaseAddressRegister::Memory { base_address } => {
                        u64::from(base_address)
                    }
                    _ => unreachable!(), // TODO: don't panic
                };

                log::info!("Initializing OHCI device at 0x{:x}", addr);
                let mut device = unsafe {
                    usb_controller_driver::ohci::init_ohci_device(HwAccess, addr)
                        .await
                        .unwrap()
                };

                for n in 1..device.root_hub_num_ports().get() {
                    let port = device.root_hub_port(NonZeroU8::new(n).unwrap()).unwrap();
                    if port.is_connected().await {
                        port.set_enabled(true).await;
                    }
                    log::info!(
                        "{:?} {:?} {:?}",
                        port.is_connected().await,
                        port.is_enabled().await,
                        port.is_suspended().await
                    );

                    device
                        .push_control(&[0x80, 0x6, 0x1, 0x0, 0x0, 0x0, 0x0, 0x12])
                        .await;
                }

                loop {
                    device.on_interrupt().await;
                }
            }
            (0xc, 0x3, 0x20) => {
                // EHCI
                unimplemented!() // TODO:
            }
            (0xc, 0x3, 0x30) => {
                // xHCI
                unimplemented!() // TODO:
            }
            _ => {}
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct HwAccess;
unsafe impl<'a> usb_controller_driver::HwAccessRef<'a> for &'a HwAccess {
    type Delay = redshirt_time_interface::Delay;
    type ReadMemFutureU8 = future::BoxFuture<'a, ()>;
    type ReadMemFutureU32 = future::BoxFuture<'a, ()>;
    type WriteMemFutureU8 = future::Ready<()>;
    type WriteMemFutureU32 = future::Ready<()>;
    type Alloc64 = future::BoxFuture<'a, Result<NonZeroU64, ()>>;
    type Alloc32 = future::BoxFuture<'a, Result<NonZeroU32, ()>>;

    unsafe fn read_memory_u8(self, address: u64, dest: &'a mut [u8]) -> Self::ReadMemFutureU8 {
        let mut builder = redshirt_hardware_interface::HardwareOperationsBuilder::new();
        builder.read(address, dest);
        builder.send().boxed()
    }

    unsafe fn read_memory_u32_be(
        self,
        address: u64,
        dest: &'a mut [u32],
    ) -> Self::ReadMemFutureU32 {
        assert_eq!(address % 4, 0); // TODO: turn into debug_assert
        let mut builder = redshirt_hardware_interface::HardwareOperationsBuilder::new();
        builder.read_u32(address, dest);
        builder.send().boxed()
    }

    unsafe fn write_memory_u8(self, address: u64, data: &[u8]) -> Self::WriteMemFutureU8 {
        redshirt_hardware_interface::write(address, data.to_vec());
        future::ready(())
    }

    unsafe fn write_memory_u32_be(self, address: u64, data: &[u32]) -> Self::WriteMemFutureU32 {
        assert_eq!(address % 4, 0); // TODO: turn into debug_assert
        let mut builder = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();
        // TODO: optimize
        for (off, elem) in data.iter().enumerate() {
            builder.write_one_u32(address + (off as u64) * 4, *elem);
        }
        builder.send();
        future::ready(())
    }

    fn alloc64(self, layout: Layout) -> Self::Alloc64 {
        redshirt_hardware_interface::malloc::malloc(
            u64::try_from(layout.size()).unwrap(),
            u64::try_from(layout.align()).unwrap(),
        )
        .map(|v| Ok(NonZeroU64::new(v).unwrap()))
        .boxed()
    }

    fn alloc32(self, layout: Layout) -> Self::Alloc32 {
        redshirt_hardware_interface::malloc::malloc(
            u64::try_from(layout.size()).unwrap(),
            u64::try_from(layout.align()).unwrap(),
        )
        .map(|v| Ok(NonZeroU32::new(u32::try_from(v).unwrap()).unwrap())) // TODO: hardware interface has no way to force 32bits allocation
        .boxed()
    }

    unsafe fn dealloc(self, address: u64, _: bool, _: Layout) {
        redshirt_hardware_interface::malloc::free(address);
    }

    fn delay(self, duration: Duration) -> Self::Delay {
        redshirt_time_interface::Delay::new(duration)
    }
}
