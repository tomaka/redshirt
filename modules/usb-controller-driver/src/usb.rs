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

use crate::{devices, ohci, HwAccessRef};

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
    Ohci(ohci::OhciDevice<TAcc>),
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

    /// Registers a new OHCI controller.
    pub async unsafe fn add_ohci(&mut self, registers: u64) -> Result<(), ohci::InitError> {
        // TODO: do the initialization in the background, otherwise we freeze all the controllers
        let ctrl = ohci::init_ohci_device(self.hardware_access.clone(), registers).await?;
        let devices = devices::UsbDevices::new(ctrl.root_hub_num_ports());
        self.controllers.push((Controller::Ohci(ctrl), devices));
        self.process(self.controllers.len() - 1).await;
        Ok(())
    }

    async fn process(&mut self, ctrl_index: usize) {
        while let Some(action) = self.controllers[ctrl_index].1.next_action() {
            match (&mut self.controllers[ctrl_index].0, action) {
                (Controller::Ohci(ref mut ctrl), devices::Action::ResetRootHubPort { port }) => {
                    ctrl.root_hub_port(port).unwrap().reset().await;
                }
                (Controller::Ohci(ref mut ctrl), devices::Action::EnableRootHubPort { port }) => {
                    ctrl.root_hub_port(port).unwrap().set_enabled(true).await;
                }
                (Controller::Ohci(ref mut ctrl), devices::Action::DisableRootHubPort { port }) => {
                    ctrl.root_hub_port(port).unwrap().set_enabled(false).await;
                }
            }
        }
    }
}
