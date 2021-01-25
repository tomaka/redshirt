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

//! OHCI handler.

use crate::{EndpointTy, HwAccessRef, PortState};
use alloc::vec::Vec;
use core::{convert::TryFrom as _, marker::PhantomData, num::NonZeroU8};
use fnv::FnvBuildHasher;
use hashbrown::HashMap;

pub use init::{init_ohci_device, InitError};
pub use transfer_descriptor::{CompletedTransferDescriptor, CompletionCode};

mod ep_descriptor;
mod ep_list;
mod hcca;
mod init;
mod registers;
mod transfer_descriptor;

pub struct OhciDevice<TAcc, TUd>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    hardware_access: TAcc,
    regs_loc: u64,
    hcca: hcca::Hcca<TAcc, (u8, u8)>,
    bulk_list: ep_list::EndpointList<TAcc, (u8, u8)>,
    control_list: ep_list::EndpointList<TAcc, (u8, u8)>,

    /// For each root hub port, the latest known status dword.
    /// In order to avoid race conditions in the API of this struct, we don't directly read the
    /// port status from the memory-mapped registers. Instead, the status is cached in this fields
    /// and refreshed after the corresponding interrupt has triggered, or after the user manually
    /// asks for a refresh.
    root_hub_ports_status: Vec<u32>,

    /// Phantom marker to pin `TUd`.
    marker: PhantomData<TUd>,
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

impl<TAcc, TUd> OhciDevice<TAcc, TUd>
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
            .write_memory_u32_le(
                config.registers_location + registers::HC_FM_INTERVAL_OFFSET,
                &[config.fm_interval_value],
            )
            .await;

        // Allocate the bulk and control lists, and set the appropriate registers.
        let mut control_list = ep_list::EndpointList::new(hardware_access.clone(), false).await;
        let bulk_list = ep_list::EndpointList::new(hardware_access.clone(), false).await;
        assert_eq!(control_list.head_pointer().get() % 16, 0);
        assert_eq!(bulk_list.head_pointer().get() % 16, 0);
        hardware_access
            .write_memory_u32_le(
                config.registers_location + registers::HC_CONTROL_HEAD_ED_OFFSET,
                &[control_list.head_pointer().get()],
            )
            .await;
        hardware_access
            .write_memory_u32_le(
                config.registers_location + registers::HC_CONTROL_CURRENT_ED_OFFSET,
                &[control_list.head_pointer().get()],
            )
            .await;
        hardware_access
            .write_memory_u32_le(
                config.registers_location + registers::HC_BULK_HEAD_ED_OFFSET,
                &[bulk_list.head_pointer().get()],
            )
            .await;
        hardware_access
            .write_memory_u32_le(
                config.registers_location + registers::HC_BULK_CURRENT_ED_OFFSET,
                &[bulk_list.head_pointer().get()],
            )
            .await;

        // Allocate the HCCA and set the appropriate register.
        let hcca = {
            // Determine the alignment requirement for the HCCA.
            let req_alignment = {
                // See section 7.2.1. We write all 1s to the HcHCCA register and read the value back.
                hardware_access
                    .write_memory_u32_le(
                        config.registers_location + registers::HC_HCCA_OFFSET,
                        &[0xffffffff],
                    )
                    .await;
                let mut out = [0];
                hardware_access
                    .read_memory_u32_le(
                        config.registers_location + registers::HC_HCCA_OFFSET,
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
            .write_memory_u32_le(
                config.registers_location + registers::HC_HCCA_OFFSET,
                &[hcca.pointer().get()],
            )
            .await;

        // Set the PeriodicStart register to around 90% of the frame interval, as described in
        // sections 5.1.1.4 and 7.3.4.
        {
            let frame_interval = config.fm_interval_value & ((1 << 14) - 1);
            let periodic_start = (9 * frame_interval) / 10;
            hardware_access
                .write_memory_u32_le(
                    config.registers_location + registers::HC_PERIODIC_START_OFFSET,
                    &[periodic_start],
                )
                .await;
        }

        // The HcControl register has some values that are set by the firmware and that must be
        // left as is. We therefore grab the current value.
        let mut hc_control_value = {
            let mut out = [0];
            hardware_access
                .read_memory_u32_le(
                    config.registers_location + registers::HC_CONTROL_OFFSET,
                    &mut out,
                )
                .await;
            // Safety check that "interrupt routing" is cleared.
            assert_eq!(out[0] & (1 << 8), 0);
            out[0]
        };

        // Enable all the queue-related bits from HcControl.
        hc_control_value |= 1 << 2; // Periodic list
        hc_control_value |= 1 << 3; // Isochronous
        hc_control_value |= 1 << 4; // Control list
        hc_control_value |= 1 << 5; // Bulk list

        // Update HcControl now. There is no fundamental reason to not combine this write with the
        // write below, but the example in the specs does it in two steps, so we do it in two
        // steps as well.
        hardware_access
            .write_memory_u32_le(
                config.registers_location + registers::HC_CONTROL_OFFSET,
                &[hc_control_value],
            )
            .await;

        // Disable all non-reserved interrupts.
        hardware_access
            .write_memory_u32_le(
                config.registers_location + registers::HC_INTERRUPT_DISABLE_OFFSET,
                &[(1 << 30) | 0b1111111],
            )
            .await;
        // Enable the master interrupt plus the ones we want.
        {
            let wanted_interrupts = 1 << 6; // Root Hub Status Change
            hardware_access
                .write_memory_u32_le(
                    config.registers_location + registers::HC_INTERRUPT_ENABLE_OFFSET,
                    &[(1 << 31) | wanted_interrupts],
                )
                .await;
        }

        // Now set it to UsbOperational.
        hc_control_value = (hc_control_value & !(0b11 << 6)) | (0b10 << 6);
        hardware_access
            .write_memory_u32_le(
                config.registers_location + registers::HC_CONTROL_OFFSET,
                &[hc_control_value],
            )
            .await;

        // Grab the number of ports on the root hub.
        let num_hub_ports = {
            let mut out = [0];
            hardware_access
                .read_memory_u32_le(
                    config.registers_location + registers::HC_RH_DESCRIPTOR_A_OFFSET,
                    &mut out,
                )
                .await;
            let v = u8::try_from(out[0] & 0xff).unwrap();
            assert_ne!(v, 0);
            v
        };

        // Get the status dwords and clear all the change bits of the root hub ports. The user is
        // supposed to query the initial state of the ports at initialization.
        let root_hub_ports_status = {
            let mut statuses: Vec<u32> = (0..num_hub_ports).map(|_| 0).collect();
            hardware_access
                .read_memory_u32_le(
                    config.registers_location + registers::HC_RH_PORT_STATUS_1_OFFSET,
                    &mut statuses,
                )
                .await;
            for n in 0..num_hub_ports {
                let addr = config.registers_location
                    + registers::HC_RH_PORT_STATUS_1_OFFSET
                    + u64::from(n) * 4;
                // We write these bits to reset the status change bits.
                let status_reset_cmd = (1 << 20) | (1 << 19) | (1 << 18) | (1 << 17) | (1 << 16);
                hardware_access
                    .write_memory_u32_le(addr, &[status_reset_cmd])
                    .await;
            }
            statuses
        };

        Self {
            hardware_access,
            regs_loc: config.registers_location,
            hcca,
            bulk_list,
            control_list,
            root_hub_ports_status,
            marker: PhantomData,
        }
    }

    /// Informs the controller that we have added new elements to either the control list, bulk
    /// list, or both.
    async fn inform_list_filled(&mut self, control_list: bool, bulk_list: bool) {
        // The `HcCommandStatus` register is a "write to set" kind of register. Writing a 1
        // activates the bit, and writing a 0 has no effect.
        let dword = if control_list { 1 << 1 } else { 0 } | if bulk_list { 1 << 2 } else { 0 };
        if dword == 0 {
            return;
        }

        unsafe {
            self.hardware_access
                .write_memory_u32_le(
                    self.regs_loc + registers::HC_COMMAND_STATUS_OFFSET,
                    &[dword],
                )
                .await;
        }
    }

    /// Reads the latest updates from the controller and returns what has happened since the last
    /// time it has been called.
    ///
    /// The host controller will generate an interrupt when something noteworthy happened, and
    /// this method should therefore be called as a result.
    pub async fn on_interrupt(&mut self) -> OnInterruptOutcome<TUd> {
        // Value to be returned at the end of this function.
        let mut outcome = OnInterruptOutcome {
            root_hub_ports_changed: false,
            completed_transfers: Default::default(),
        };

        // Read the `InterruptStatus` register, indicating what has happened since the last read.
        let interrupt_status = unsafe {
            let mut out = [0];
            self.hardware_access
                .read_memory_u32_le(
                    self.regs_loc + registers::HC_INTERRUPT_STATUS_OFFSET,
                    &mut out,
                )
                .await;
            out[0]
        };

        // WriteBackDoneHead
        // The controller has updated the done queue in the HCCA.
        if interrupt_status & (1 << 1) != 0 {
            let list = unsafe { self.hcca.extract_done_queue::<TUd>().await };
            // TODO: need to handle the possible Halted bit
            // TODO: remove these debug things
            log::info!(
                "completed: {:?}",
                list.iter()
                    .map(|l| l.completion_code.clone())
                    .collect::<Vec<_>>()
            );
            log::info!(
                "completed: {:?}",
                list.iter()
                    .map(|l| l.buffer_back.clone())
                    .collect::<Vec<_>>()
            );
            outcome.completed_transfers.extend(list);
        }

        // RootHubStatusChange
        // One or more devices in the root hub have changed status.
        if interrupt_status & (1 << 6) != 0 {
            outcome.root_hub_ports_changed = true;

            // TODO: this doesn't clear the status change bits; do we care?
            // Refresh `root_hub_ports_status`.
            unsafe {
                self.hardware_access
                    .read_memory_u32_le(
                        self.regs_loc + registers::HC_RH_PORT_STATUS_1_OFFSET,
                        &mut self.root_hub_ports_status,
                    )
                    .await;
            }
        }

        // UnrecoverableError
        // A system error not related to USB has been detected.
        if interrupt_status & (1 << 4) != 0 {
            panic!() // TODO:
        }

        // Clear all interrupt status flags.
        // TODO: what if something more has happened in between? need to loop or something
        unsafe {
            self.hardware_access
                .write_memory_u32_le(
                    self.regs_loc + registers::HC_INTERRUPT_STATUS_OFFSET,
                    &[interrupt_status],
                )
                .await;
        }

        outcome
    }

    /// Access a port of the root hub.
    ///
    /// Returns `None` if `port` is out of range.
    ///
    /// Just like regular USB hubs, ports indexing starts from 1.
    pub fn root_hub_port(&mut self, port: NonZeroU8) -> Option<RootHubPort<TAcc, TUd>> {
        if usize::from(port.get()) >= self.root_hub_ports_status.len() + 1 {
            return None;
        }

        Some(RootHubPort {
            controller: self,
            port,
        })
    }

    /// Returns the number of ports in the root hub. Never changes.
    pub fn root_hub_num_ports(&self) -> NonZeroU8 {
        NonZeroU8::new(u8::try_from(self.root_hub_ports_status.len()).unwrap()).unwrap()
    }

    /// Access a specific endpoint at an address.
    pub fn endpoint<'a>(
        &'a mut self,
        function_address: u8,
        endpoint_number: u8,
    ) -> Endpoint<'a, TAcc, TUd> {
        // TODO: stronger typing?
        assert!(function_address < 128);
        assert!(endpoint_number < 16);

        if self
            .bulk_list
            .find_by_user_data(|d| *d == (function_address, endpoint_number))
            .is_some()
        {
            return Endpoint::Known(KnownEndpoint {
                controller: self,
                function_address,
                endpoint_number,
                ty: EndpointTy::Bulk,
            });
        }

        if self
            .control_list
            .find_by_user_data(|d| *d == (function_address, endpoint_number))
            .is_some()
        {
            return Endpoint::Known(KnownEndpoint {
                controller: self,
                function_address,
                endpoint_number,
                ty: EndpointTy::Control,
            });
        }

        // TODO: isochronous and interrupt too

        Endpoint::Unknown(UnknownEndpoint {
            controller: self,
            function_address,
            endpoint_number,
        })
    }
}

/// Outcome of calling [`OhciDevice::on_interrupt`].
#[derive(Debug)]
#[must_use]
pub struct OnInterruptOutcome<TUd> {
    /// True if any of the root hub ports status has changed.
    pub root_hub_ports_changed: bool,

    /// List of transfers that have finished.
    pub completed_transfers: Vec<CompletedTransferDescriptor<TUd>>,
}

pub enum Endpoint<'a, TAcc, TUd>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    Unknown(UnknownEndpoint<'a, TAcc, TUd>),
    Known(KnownEndpoint<'a, TAcc, TUd>),
}

impl<'a, TAcc, TUd> Endpoint<'a, TAcc, TUd>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Returns a [`KnownEndpoint`] if this endpoint is known.
    pub fn into_known(self) -> Option<KnownEndpoint<'a, TAcc, TUd>> {
        match self {
            Endpoint::Known(v) => Some(v),
            _ => None,
        }
    }

    /// Returns an [`UnknownEndpoint`] if this endpoint is unknown.
    pub fn into_unknown(self) -> Option<UnknownEndpoint<'a, TAcc, TUd>> {
        match self {
            Endpoint::Unknown(v) => Some(v),
            _ => None,
        }
    }
}

pub struct UnknownEndpoint<'a, TAcc, TUd>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    controller: &'a mut OhciDevice<TAcc, TUd>,
    function_address: u8,
    endpoint_number: u8,
}

impl<'a, TAcc, TUd> UnknownEndpoint<'a, TAcc, TUd>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Inserts the endpoint in the list of endpoints.
    pub async fn insert(self, ty: EndpointTy) -> KnownEndpoint<'a, TAcc, TUd> {
        let config = ep_list::Config {
            maximum_packet_size: 4095, // TODO: ?
            function_address: self.function_address,
            endpoint_number: self.endpoint_number,
            isochronous: matches!(ty, EndpointTy::Isochronous),
            low_speed: true,                       // TODO: ?
            direction: ep_list::Direction::FromTd, // TODO: ?
        };

        match ty {
            EndpointTy::Bulk => {
                self.controller
                    .bulk_list
                    .push(config, (self.function_address, self.endpoint_number))
                    .await
            }
            EndpointTy::Control => {
                self.controller
                    .control_list
                    .push(config, (self.function_address, self.endpoint_number))
                    .await
            }
            EndpointTy::Isochronous => unimplemented!(),
            EndpointTy::Interrupt => unimplemented!(),
        };

        KnownEndpoint {
            controller: self.controller,
            function_address: self.function_address,
            endpoint_number: self.endpoint_number,
            ty,
        }
    }
}

pub struct KnownEndpoint<'a, TAcc, TUd>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    controller: &'a mut OhciDevice<TAcc, TUd>,
    function_address: u8,
    endpoint_number: u8,
    ty: EndpointTy,
}

impl<'a, TAcc, TUd> KnownEndpoint<'a, TAcc, TUd>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
    TUd: 'static,
{
    /// Removes the endpoint from the list. We're not going to use it anymore.
    pub async fn remove(self) {
        unimplemented!()
    }

    pub async fn send(&mut self, data: &[u8], user_data: TUd) {
        self.send_inner(false, data, user_data).await
    }

    pub async fn send_setup(&mut self, data: &[u8], user_data: TUd) {
        self.send_inner(true, data, user_data).await
    }

    async fn send_inner(&mut self, setup: bool, data: &[u8], user_data: TUd) {
        let expected_ud = (self.function_address, self.endpoint_number);
        assert_eq!(self.ty, EndpointTy::Control); // TODO: not implemented otherwise
        self.controller
            .control_list
            .find_by_user_data(|d| *d == expected_ud)
            .unwrap()
            .push_packet(
                ep_list::TransferDescriptorConfig::GeneralOut {
                    data,
                    setup,
                    delay_interrupt: 0,
                },
                user_data,
            )
            .await;
        self.controller.inform_list_filled(true, false).await;
    }

    /// Adds a new "IN" transfer descriptor to the queue, meaning that we wait for a packet from
    /// the endpoint.
    pub async fn receive(&mut self, buffer_len: u16, user_data: TUd) {
        let expected_ud = (self.function_address, self.endpoint_number);
        assert_eq!(self.ty, EndpointTy::Control); // TODO: not implemented otherwise
        self.controller
            .control_list
            .find_by_user_data(|d| *d == expected_ud)
            .unwrap()
            .push_packet(
                ep_list::TransferDescriptorConfig::GeneralIn {
                    buffer_len: usize::from(buffer_len),
                    buffer_rounding: false, // TODO: ?
                    delay_interrupt: 0,
                },
                user_data,
            )
            .await;
        self.controller.inform_list_filled(true, false).await;
    }
}

/// Access to a port of the root hub of the controller.
pub struct RootHubPort<'a, TAcc, TUd>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    controller: &'a mut OhciDevice<TAcc, TUd>,
    port: NonZeroU8,
}

impl<'a, TAcc, TUd> RootHubPort<'a, TAcc, TUd>
where
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    async fn write_status(&self, dword: u32) {
        unsafe {
            let addr = self.controller.regs_loc
                + registers::HC_RH_PORT_STATUS_1_OFFSET
                + u64::from(self.port.get() - 1) * 4;
            self.controller
                .hardware_access
                .write_memory_u32_le(addr, &[dword])
                .await;
        }
    }

    /// Returns the state of this port.
    ///
    /// > **Note**: The returned value is cached, and calling this function multiple times in a
    /// >           row will always return the same value. Call [`OhciController::on_interrupt`]
    /// >           to observe modifications.
    pub fn state(&self) -> PortState {
        let status_dword = self.controller.root_hub_ports_status[usize::from(self.port.get() - 1)];
        let power_bit = status_dword & (1 << 8) != 0;
        let connected_bit = status_dword & (1 << 0) != 0;
        let enabled_bit = status_dword & (1 << 1) != 0;
        let suspended_bit = status_dword & (1 << 2) != 0;
        let reset_bit = status_dword & (1 << 4) != 0;

        match (
            power_bit,
            connected_bit,
            reset_bit,
            enabled_bit,
            suspended_bit,
        ) {
            (false, false, false, false, false) => PortState::NotPowered,
            (true, false, false, false, false) => PortState::Disconnected,
            (true, true, false, false, false) => PortState::Disabled,
            (true, true, true, false, false) => PortState::Resetting,
            (true, true, false, true, false) => PortState::Enabled,
            // Note that the specs are a bit ambiguous about whether `enabled_bit` is true when
            // the port is suspended.
            (true, true, false, _, true) => PortState::Suspended,
            // Note that resuming is instantaneous and therefore never reported.
            _ => unreachable!(),
        }
    }

    /// Modifies the status of this port.
    ///
    /// # Panic
    ///
    /// Panics if the new status doesn't make sense as a transition from the old status. The
    /// allowed transitions are:
    ///
    /// - Disabled => Resetting
    /// - Enabled => Disabled
    /// - Resetting => Disabled
    /// - Suspended => Disabled
    /// - Resuming => Disabled
    /// - Enabled => Suspended
    /// - Suspended => Resuming
    ///
    /// Note that the `NotPowered => Disconnected` transition, while it normally makes sense, is
    /// not supported by the OHCI controller.
    pub async fn set_state(&mut self, new_status: PortState) {
        // Note that it is possible that `self.state()` returns obsolete information and that this
        // is a race condition. However this race condition exists in the specs in general. It is
        // not possible to atomatically read the current state and perform an action at the same
        // time.
        match (self.state(), new_status) {
            (PortState::Disabled, PortState::Resetting) => self.write_status(1 << 4).await,
            (PortState::Enabled, PortState::Disabled)
            | (PortState::Resetting, PortState::Disabled)
            | (PortState::Suspended, PortState::Disabled)
            | (PortState::Resuming, PortState::Disabled) => self.write_status(1 << 0).await,
            (PortState::Enabled, PortState::Suspended) => self.write_status(1 << 2).await,
            (PortState::Suspended, PortState::Resuming) => self.write_status(1 << 3).await,
            (from, to) => panic!("cannot transition from {:?} to {:?}", from, to),
        }
    }
}
