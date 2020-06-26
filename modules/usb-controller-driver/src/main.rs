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

use core::{
    alloc::Layout,
    convert::TryFrom as _,
    num::{NonZeroU32, NonZeroU64},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use futures::prelude::*;

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    let mut usb_state = usb_controller_driver::Usb::new(HwAccess);
    let mut pci_devices_locks = Vec::new();

    // Try to find USB host controllers in the list of PCI devices.
    for device in redshirt_pci_interface::get_pci_devices().await {
        match (device.class_code, device.subclass, device.prog_if) {
            (0xc, 0x3, 0x0) => {
                // UHCI
                unimplemented!() // TODO:
            }
            (0xc, 0x3, 0x10) => {
                // OHCI
                let addr = match device.base_address_registers[0] {
                    redshirt_pci_interface::PciBaseAddressRegister::Memory { base_address } => {
                        u64::from(base_address)
                    }
                    _ => {
                        log::error!("Found OHCI PCI controller with non-memory BAR0.");
                        continue;
                    }
                };

                // Try to lock the given PCI device. Can fail if there is another USB controller
                // drive that is already handling the device.
                let lock = match redshirt_pci_interface::PciDeviceLock::lock(device.location).await
                {
                    Ok(l) => l,
                    Err(_) => continue,
                };
                // TODO: should write to LATENCY_TIMER in the PCI config space, as the specs mention
                lock.set_command(true, true, false);

                if let Err(err) = unsafe { usb_state.add_ohci(addr).await } {
                    log::error!(
                        "Failed to initialize OHCI host controller at 0x{:x}: {}",
                        addr,
                        err
                    );
                    continue;
                }

                log::info!("Initialized OHCI device at 0x{:x}", addr);
                pci_devices_locks.push(lock);
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

    // If no device has been found, exit the program now. The list of PCI devices can never change.
    if pci_devices_locks.is_empty() {
        return;
    }

    // TODO: as a hack before interrupts are supported, we just periodically call `on_interrupt`
    loop {
        {
            let interrupt = redshirt_time_interface::Delay::new(Duration::from_millis(1));
            let next_event = usb_state.next_event();
            futures::pin_mut!(interrupt, next_event);
            match future::select(interrupt, next_event).await {
                future::Either::Left(((), _)) => {}
                future::Either::Right((
                    usb_controller_driver::usb::Event::ProcessingRequired(p),
                    _,
                )) => p.process().await,
            }
        }
        usb_state.on_interrupt().await;
    }
}

/// Implementation of [`usb_controller_driver::HwAccessRef`]. Makes it possible for the library to
/// communicate with redshirt.
#[derive(Debug, Copy, Clone)]
struct HwAccess;

unsafe impl<'a> usb_controller_driver::HwAccessRef<'a> for &'a HwAccess {
    type Delay = redshirt_time_interface::Delay;
    type ReadMemFutureU8 = future::BoxFuture<'a, ()>;
    type ReadMemFutureU32 = future::BoxFuture<'a, ()>;
    type WriteMemFutureU8 = WriteMemoryFutureU8<'a>;
    type WriteMemFutureU32 = WriteMemoryFutureU32<'a>;
    type Alloc64 = future::BoxFuture<'a, Result<NonZeroU64, ()>>;
    type Alloc32 = future::BoxFuture<'a, Result<NonZeroU32, ()>>;

    unsafe fn read_memory_u8(self, address: u64, dest: &'a mut [u8]) -> Self::ReadMemFutureU8 {
        let mut builder = redshirt_hardware_interface::HardwareOperationsBuilder::new();
        builder.read(address, dest);
        builder.send().boxed()
    }

    // TODO: enforce the endianess
    unsafe fn read_memory_u32_le(
        self,
        address: u64,
        dest: &'a mut [u32],
    ) -> Self::ReadMemFutureU32 {
        debug_assert_eq!(address % 4, 0);
        let mut builder = redshirt_hardware_interface::HardwareOperationsBuilder::new();
        builder.read_u32(address, dest);
        builder.send().boxed()
    }

    unsafe fn write_memory_u8(self, address: u64, data: &'a [u8]) -> Self::WriteMemFutureU8 {
        WriteMemoryFutureU8 { address, data }
    }

    // TODO: enforce the endianess
    unsafe fn write_memory_u32_le(self, address: u64, data: &'a [u32]) -> Self::WriteMemFutureU32 {
        debug_assert_eq!(address % 4, 0);
        WriteMemoryFutureU32 { address, data }
    }

    fn alloc64(self, layout: Layout) -> Self::Alloc64 {
        // TODO: leaks if future is cancelled
        redshirt_hardware_interface::malloc::malloc(
            u64::try_from(layout.size()).unwrap(),
            u64::try_from(layout.align()).unwrap(),
        )
        .map(|v| Ok(NonZeroU64::new(v).unwrap()))
        .boxed()
    }

    fn alloc32(self, layout: Layout) -> Self::Alloc32 {
        // TODO: leaks if future is cancelled
        // TODO: hardware interface has no way to force 32bits allocation
        redshirt_hardware_interface::malloc::malloc(
            u64::try_from(layout.size()).unwrap(),
            u64::try_from(layout.align()).unwrap(),
        )
        .map(|v| Ok(NonZeroU32::new(u32::try_from(v).unwrap()).unwrap()))
        .boxed()
    }

    unsafe fn dealloc(self, address: u64, _: bool, _: Layout) {
        redshirt_hardware_interface::malloc::free(address);
    }

    fn delay(self, duration: Duration) -> Self::Delay {
        redshirt_time_interface::Delay::new(duration)
    }
}

struct WriteMemoryFutureU8<'a> {
    address: u64,
    data: &'a [u8],
}

impl<'a> Future for WriteMemoryFutureU8<'a> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        unsafe {
            redshirt_hardware_interface::write(self.address, self.data.to_vec());
            Poll::Ready(())
        }
    }
}

struct WriteMemoryFutureU32<'a> {
    address: u64,
    data: &'a [u32],
}

impl<'a> Future for WriteMemoryFutureU32<'a> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        unsafe {
            let mut builder = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();
            // TODO: optimize
            for (off, elem) in self.data.iter().enumerate() {
                builder.write_one_u32(self.address + (off as u64) * 4, *elem);
            }
            builder.send();
            Poll::Ready(())
        }
    }
}
