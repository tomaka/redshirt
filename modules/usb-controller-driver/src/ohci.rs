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

//! OHCI handler.

use crate::HwAccessRef;
use core::convert::TryFrom as _;

pub use init::init_ohci_device;

mod definitions;
mod ep_descriptor;
mod ep_list;
mod init;

pub struct OhciDevice<TAcc> {
    hardware_access: TAcc,
    regs_loc: u64,
}

/// Information about the suspended device.
#[derive(Debug)]
pub(crate) struct FromSuspendedConfig {
    /// Location of the memory-mapped registers in physical memory.
    pub registers_location: u64,

    /// Value of the `FmInterval` register that must be configured for this device.
    /// This is normally set by the firmware, and read by the driver before the controller is
    /// reset.
    pub fm_interval_value: u32,
}

impl<TAcc> OhciDevice<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Initializes an [`OhciDevice`] that is in a suspended state.
    ///
    /// The `HostControllerFunctionalState` value of this device must be `UsbSuspend` (0b11).
    pub(crate) async unsafe fn from_suspended(
        hardware_access: TAcc,
        config: FromSuspendedConfig,
    ) -> Self {
        // See section 5.1.1.4.

        // TODO: somehow deal with this "should not stay in this state more than 2ms" requirement?
        //       If this function takes more than 2ms, then all devices will think that the hub
        //       is gone and will switch to sleep mode, which would be bad. See section 5.1.2.3.
    
        // Set the `FmInterval` register.
        hardware_access
            .write_memory_u32(
                config.registers_location + definitions::HC_FM_INTERVAL_OFFSET,
                &[config.fm_interval_value],
            )
            .await;

        // TODO: move somewhere else
        let num_hub_ports = {
            let mut out = [0];
            hardware_access
                .read_memory_u32(
                    config.registers_location + definitions::HC_RH_DESCRIPTOR_A_OFFSET,
                    &mut out,
                )
                .await;
            u8::try_from(out[0] & 0xff).unwrap()
        };

        // Allocating the HCCA buffer.
        let hcca = {
            // Determine the alignment requirement for the HCCA.
            let req_alignment = {
                // See section 7.2.1. We write all 1s to the HcHCCA register and read the value back.
                hardware_access
                    .write_memory_u32(
                        config.registers_location + definitions::HC_HCCA_OFFSET,
                        &[0xffffffff],
                    )
                    .await;
                let mut out = [0];
                hardware_access
                    .read_memory_u32(
                        config.registers_location + definitions::HC_HCCA_OFFSET,
                        &mut out,
                    )
                    .await;
                // The value of HC_HCCA will be something like `111..11110000`. We count the
                // number of trailing 0s.
                1u64 << out[0].trailing_zeros()
            };
        };

        Self {
            hardware_access,
            regs_loc: config.registers_location,
        }
    }
}
