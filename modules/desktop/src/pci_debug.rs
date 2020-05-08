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

use crate::rasterizer::Rasterizer;

/// PCI debug widget.
pub struct PciDebug {
    devices: Vec<redshirt_pci_interface::PciDeviceInfo>,
}

impl PciDebug {
    /// Registers resources towards the rasterizer.
    pub async fn new(rasterizer: &mut Rasterizer) -> Self {
        PciDebug {
            devices: redshirt_pci_interface::get_pci_devices().await,
        }
    }

    /// Draws the widget on the UI.
    ///
    /// Must then be rendered with the rasterized that was passed at initialization.
    pub fn draw(&mut self, ui: &imgui::Ui) {
        imgui::Window::new(imgui::im_str!("pci-debug"))
            .opened(&mut true)
            .build(&ui, || {
                for device in &self.devices {
                    ui.bullet_text(imgui::im_str!("PCI device here"));
                }
            });
    }
}
