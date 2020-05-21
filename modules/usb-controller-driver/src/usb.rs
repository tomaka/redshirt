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

use crate::{devices, ohci, HwAccessRef, PortState};

use core::num::NonZeroU8;
use smallvec::SmallVec;

/// Manages the state of all USB host controllers and their devices.
pub struct Usb<TAcc>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Hardware access for new controllers.
    hardware_access: TAcc,

    /// List of controllers.
    controllers: SmallVec<[(Controller<TAcc>, devices::UsbDevices); 4]>,
}

enum Controller<TAcc>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    Ohci(ohci::OhciDevice<TAcc, devices::PacketId>),
}

impl<TAcc> Usb<TAcc>
where
    TAcc: Clone,
    for<'r> &'r TAcc: HwAccessRef<'r>,
{
    /// Initializes a new empty state.
    pub fn new(hardware_access: TAcc) -> Self {
        Self {
            hardware_access,
            controllers: SmallVec::new(),
        }
    }

    /// Reads the latest updates from the controllers.
    ///
    /// Host controllers will generate an interrupt when something noteworthy happened, and this
    /// method should therefore be called as a result.
    // TODO: pass as parameter some sort of identifier for the controller that has interrupted?
    pub async fn on_interrupt(&mut self) {
        let mut updated_controllers = SmallVec::<[_; 4]>::new();

        for (ctrl_index, (ctrl, usb_devices)) in self.controllers.iter_mut().enumerate() {
            match ctrl {
                Controller::Ohci(ctrl) => {
                    let outcome = ctrl.on_interrupt().await;
                    if outcome.root_hub_ports_changed {
                        for port_num in 0..ctrl.root_hub_num_ports().get() {
                            let port_num = NonZeroU8::new(port_num + 1).unwrap();
                            let port = ctrl.root_hub_port(port_num).unwrap();
                            usb_devices.set_root_hub_port_state(port_num, port.state());
                        }
                        updated_controllers.push(ctrl_index);
                    }
                }
            }
        }

        for ctrl_index in updated_controllers {
            self.process(ctrl_index).await;
        }
    }

    /// Registers a new OHCI controller.
    pub async unsafe fn add_ohci(&mut self, registers: u64) -> Result<(), ohci::InitError> {
        // TODO: do the initialization in the background, otherwise we freeze all the controllers
        let mut ctrl = ohci::init_ohci_device(self.hardware_access.clone(), registers).await?;
        let mut devices = devices::UsbDevices::new(ctrl.root_hub_num_ports());
        for port_num in 0..ctrl.root_hub_num_ports().get() {
            let port_num = NonZeroU8::new(port_num + 1).unwrap();
            let port = ctrl.root_hub_port(port_num).unwrap();
            devices.set_root_hub_port_state(port_num, port.state());
        }
        self.controllers.push((Controller::Ohci(ctrl), devices));
        self.process(self.controllers.len() - 1).await;
        Ok(())
    }

    async fn process(&mut self, ctrl_index: usize) {
        while let Some(action) = self.controllers[ctrl_index].1.next_action() {
            match (&mut self.controllers[ctrl_index].0, action) {
                (
                    Controller::Ohci(ref mut ctrl),
                    devices::Action::SetRootHubPortState { port, state },
                ) => {
                    ctrl.root_hub_port(port).unwrap().set_state(state).await;
                }

                (
                    Controller::Ohci(ref mut ctrl),
                    devices::Action::AllocateNewEndpoint {
                        function_address,
                        endpoint_number,
                        ty,
                    },
                ) => {
                    ctrl.endpoint(function_address, endpoint_number)
                        .into_unknown()
                        .unwrap()
                        .insert(ty)
                        .await;
                }

                (
                    Controller::Ohci(ref mut ctrl),
                    devices::Action::FreeEndpoint {
                        function_address,
                        endpoint_number,
                    },
                ) => {
                    ctrl.endpoint(function_address, endpoint_number)
                        .into_known()
                        .unwrap()
                        .remove()
                        .await;
                }

                (
                    Controller::Ohci(ref mut ctrl),
                    devices::Action::EmitInPacket {
                        id,
                        function_address,
                        endpoint_number,
                        buffer_len,
                    },
                ) => {
                    ctrl.endpoint(function_address, endpoint_number)
                        .into_known()
                        .unwrap()
                        .receive(buffer_len, id)
                        .await;
                }

                (
                    Controller::Ohci(ref mut ctrl),
                    devices::Action::EmitOutPacket {
                        id,
                        function_address,
                        endpoint_number,
                        ref data,
                    },
                ) => {
                    ctrl.endpoint(function_address, endpoint_number)
                        .into_known()
                        .unwrap()
                        .send(data, id)
                        .await;
                }

                (
                    Controller::Ohci(ref mut ctrl),
                    devices::Action::EmitSetupPacket {
                        id,
                        function_address,
                        endpoint_number,
                        data,
                    },
                ) => {
                    ctrl.endpoint(function_address, endpoint_number)
                        .into_known()
                        .unwrap()
                        .send_setup(&data, id)
                        .await;
                }
            }
        }
    }
}
