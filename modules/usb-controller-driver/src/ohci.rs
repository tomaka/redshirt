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
mod hcca;
mod init;
mod transfer_descriptor;

pub struct OhciDevice<TAcc>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    hardware_access: TAcc,
    regs_loc: u64,
    bulk_list: ep_list::EndpointList<TAcc>,
    control_list: ep_list::EndpointList<TAcc>,
    hc_control_value: u32,
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
    TAcc: Clone,
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
            .write_memory_u32_be(
                config.registers_location + definitions::HC_FM_INTERVAL_OFFSET,
                &[config.fm_interval_value],
            )
            .await;

        // TODO: move somewhere else
        let num_hub_ports = {
            let mut out = [0];
            hardware_access
                .read_memory_u32_be(
                    config.registers_location + definitions::HC_RH_DESCRIPTOR_A_OFFSET,
                    &mut out,
                )
                .await;
            u8::try_from(out[0] & 0xff).unwrap()
        };

        // Allocate the bulk and control lists, and set the appropriate registers.
        let control_list = ep_list::EndpointList::new(hardware_access.clone()).await;
        let bulk_list = ep_list::EndpointList::new(hardware_access.clone()).await;
        assert_eq!(control_list.head_pointer().get() % 16, 0);
        assert_eq!(bulk_list.head_pointer().get() % 16, 0);
        hardware_access
            .write_memory_u32_be(
                config.registers_location + definitions::HC_CONTROL_HEAD_ED_OFFSET,
                &[control_list.head_pointer().get()],
            )
            .await;
        hardware_access
            .write_memory_u32_be(
                config.registers_location + definitions::HC_CONTROL_CURRENT_ED_OFFSET,
                &[control_list.head_pointer().get()],
            )
            .await;
        hardware_access
            .write_memory_u32_be(
                config.registers_location + definitions::HC_BULK_HEAD_ED_OFFSET,
                &[bulk_list.head_pointer().get()],
            )
            .await;
        hardware_access
            .write_memory_u32_be(
                config.registers_location + definitions::HC_BULK_CURRENT_ED_OFFSET,
                &[bulk_list.head_pointer().get()],
            )
            .await;

        // Allocate the HCCA and set the appropriate register.
        let hcca = {
            // Determine the alignment requirement for the HCCA.
            let req_alignment = {
                // See section 7.2.1. We write all 1s to the HcHCCA register and read the value back.
                hardware_access
                    .write_memory_u32_be(
                        config.registers_location + definitions::HC_HCCA_OFFSET,
                        &[0xffffffff],
                    )
                    .await;
                let mut out = [0];
                hardware_access
                    .read_memory_u32_be(
                        config.registers_location + definitions::HC_HCCA_OFFSET,
                        &mut out,
                    )
                    .await;
                // The value of HC_HCCA will be something like `111..11110000`. We count the
                // number of trailing 0s.
                1u64 << out[0].trailing_zeros()
            };

            hcca::Hcca::new(
                hardware_access.clone(),
                usize::try_from(req_alignment).unwrap(),
            )
            .await
        };
        assert_eq!(hcca.pointer().get() % 16, 0);
        hardware_access
            .write_memory_u32_be(
                config.registers_location + definitions::HC_HCCA_OFFSET,
                &[hcca.pointer().get()],
            )
            .await;

        // Set the PeriodicStart register to around 90% of the frame interval, as described in
        // sections 5.1.1.4 and 7.3.4.
        {
            let frame_interval = config.fm_interval_value & ((1 << 14) - 1);
            let periodic_start = (9 * frame_interval) / 10;
            hardware_access
                .write_memory_u32_be(
                    config.registers_location + definitions::HC_PERIODIC_START_OFFSET,
                    &[periodic_start],
                )
                .await;
        }

        // The HcControl register has some values that are set by the firmware and that must be
        // left as is. We therefore grab the current value.
        let mut hc_control_value = {
            let mut out = [0];
            hardware_access
                .read_memory_u32_be(
                    config.registers_location + definitions::HC_CONTROL_OFFSET,
                    &mut out,
                )
                .await;
            out[0]
        };
        // Safety check that "interrupt routing" is cleared.
        assert_eq!(hc_control_value & (1 << 8), 0);

        // Enable all the queue-related bits from HcControl.
        hc_control_value |= 1 << 2; // Periodic list
        hc_control_value |= 1 << 3; // Isochronous
        hc_control_value |= 1 << 4; // Control list
        hc_control_value |= 1 << 5; // Bulk list

        // Update HcControl now. There is no fundamental reason to not combine this write with the
        // write below, but the example in the specs does it in two steps, so we do it in two
        // steps as well.
        hardware_access
            .write_memory_u32_be(
                config.registers_location + definitions::HC_CONTROL_OFFSET,
                &[hc_control_value],
            )
            .await;

        // Now set it to UsbOperational.
        hc_control_value = (hc_control_value & !(0b11 << 6)) | (0b10 << 6);
        hardware_access
            .write_memory_u32_be(
                config.registers_location + definitions::HC_CONTROL_OFFSET,
                &[hc_control_value],
            )
            .await;

        // TODO: remove this
        log::info!("initialized");

        Self {
            hardware_access,
            regs_loc: config.registers_location,
            bulk_list,
            control_list,
            hc_control_value,
        }
    }
}
