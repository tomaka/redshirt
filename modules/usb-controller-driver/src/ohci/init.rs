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

//! OHCI initialization.
//!
//! Initializes an OCHI implementation.
//!
//! Because of legacy compatibility, the OHCI can be in three possible states when the operating
//! system starts:
//!
//! - Used by the System Management Mode driver (SMM). The SMM driver is one of the first
//! components that starts at system initialization, and redirects the legacy PS/2 ports to the
//! USB controller. If the SMM driver has ownership of the OHCI controller, the `InterruptRouting`
//! bit is set in the `HcControl` register.
//! - Used by the BIOS, or by a previous operating system driver. If the BIOS has ownership of the
//! OCHI controller, the `InterruptRouting` bit is not set and the `HostControllerFunctionalState`
//! is not `UsbReset`.
//! - Not powered up. The `InterruptRouting` bit is not set and `HostControllerFunctionalState` is
//! `UsbReset`.
//!
//! See also section 5.1.1.3 of the specs.
//!
//! This module performs all the necessary resets and switches the OHCI controller to.
// TODO: finish stating in which mode we switch to

use crate::{
    ohci::{definitions, FromSuspendedConfig, OhciDevice},
    HwAccessRef,
};
use core::convert::TryFrom as _;

/// Error that can happen during initialization.
#[derive(Debug, derive_more::Display)]
pub enum InitError {
    /// Unrecognized driver revision number.
    ///
    /// > **Note**: This probably indicates that the memory location doesn't correspond to an
    /// >           OHCI implementation, or that there is a bug in the physical memory access
    /// >           mechanism.
    BadRevision(u8),
}

/// Initializes an OHCI device whose registers are memory-mapped at the given location.
pub async unsafe fn init_ohci_device<TAcc>(
    access: TAcc,
    regs_loc: u64,
) -> Result<OhciDevice<TAcc>, InitError>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    // See section 5.1.1.2. We start by checking whether the revision is one we know.
    let revision = {
        let mut out = [0];
        access
            .read_memory_u32(regs_loc + definitions::HC_REVISION_OFFSET, &mut out)
            .await;
        u8::try_from(out[0] & 0xff).unwrap()
    };
    if revision != 0x10 {
        return Err(InitError::BadRevision(revision));
    }

    // Reading the `HcControl` register to determine in which mode the controller is.
    // See section 7.1.2.
    let (functional_state, interrupt_routing) = {
        let mut out = [0];
        access
            .read_memory_u32(regs_loc + definitions::HC_CONTROL_OFFSET, &mut out)
            .await;
        let interrupt_routing = (out[0] & (1 << 8)) != 0;
        let functional_state = u8::try_from((out[0] >> 6) & 0b11).unwrap();
        (functional_state, interrupt_routing)
    };

    match (functional_state, interrupt_routing) {
        (_, true) => {
            // Owned by SMM driver.
            // See section 5.1.1.3.3.

            // We write 1 to the `OwnershipChangerRequest` flag of the command register to ask
            // the SMM to stop using the controller.
            access
                .write_memory_u32(
                    regs_loc + definitions::HC_COMMAND_STATUS_OFFSET,
                    &[1u32 << 3u32],
                )
                .await;

            // Now looping until `interrupt_routing` is 1.
            loop {
                // TODO: put a sleep here

                let mut out = [0];
                access
                    .read_memory_u32(regs_loc + definitions::HC_CONTROL_OFFSET, &mut out)
                    .await;
                let interrupt_routing = (out[0] & (1 << 8)) != 0;
                if !interrupt_routing {
                    break;
                }
            }
        }
        (0b00, false) => {
            // Controller is in `UsbReset` mode and isn't initialized yet.
            // See section 5.1.1.3.5.
            // TODO: wait some time, as described in the specs
        }
        (0b10, false) => {
            // Controller is in `UsbOperational` mode. It was in use by the BIOS or a previous
            // driver.
            // See section 5.1.1.3.4.
            // There is nothing more to do here, and we directly move on to resetting the
            // controller.
        }
        (0b01, false) | (0b11, false) => {
            // Controller is not in `UsbReset` mode and was in use by the BIOS or a previous
            // driver.
            // See section 5.1.1.3.4.
            unimplemented!() // TODO: do the thing the specs say
        }
        (_, _) => unreachable!(),
    }

    // See section 5.1.1.4 for the rest of the body.

    // We now save the value of the `HcFmInterval` register. It is sometimes set by the firmware
    // at system initialization. The reset we perform below will erase its value, and we need to
    // restore the value afterwards.
    let fm_interval_value = {
        let mut out = [0];
        access
            .read_memory_u32(regs_loc + definitions::HC_FM_INTERVAL_OFFSET, &mut out)
            .await;
        out[0]
    };

    // We write 1 to the `HostControllerReset` flag of the command register to reset the
    // controller.
    access
        .write_memory_u32(
            regs_loc + definitions::HC_COMMAND_STATUS_OFFSET,
            &[1u32 << 0u32],
        )
        .await;
    // The reset lasts for a maximum of 10µs, as described in specs.
    // TODO: wait for 10µs

    Ok({
        let cfg = FromSuspendedConfig {
            registers_location: regs_loc,
            fm_interval_value,
        };

        OhciDevice::from_suspended(access, cfg).await
    })
}