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

use futures::prelude::*;
use std::pin::Pin;

/// PCI debug widget.
pub struct PciDebug {
    devices: future::MaybeDone<Pin<Box<dyn Future<Output = Vec<redshirt_pci_interface::PciDeviceInfo>>>>>,
}

impl PciDebug {
    /// Registers resources towards the rasterizer.
    pub fn new(_rasterizer: &mut Rasterizer) -> Self {
        PciDebug {
            // TODO: blocks forever
            // future::maybe_done(Box::pin(redshirt_pci_interface::get_pci_devices())),
            devices: future::MaybeDone::Gone,
        }
    }

    /// Draws the widget on the UI.
    ///
    /// Must then be rendered with the rasterized that was passed at initialization.
    pub fn draw(&mut self, ui: &imgui::Ui) {
        (&mut self.devices).now_or_never();
        let devices = match &self.devices {
            future::MaybeDone::Done(d) => d,
            _ => return
        };

        imgui::Window::new(imgui::im_str!("pci-debug"))
            .opened(&mut true)
            .build(&ui, || {
                for device in devices {
                    ui.bullet_text(imgui::im_str!("PCI device here"));
                }
            });
    }
}
