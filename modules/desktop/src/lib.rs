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

use std::time::Instant;

mod background;
mod launch_bar;
mod pci_debug;
mod rasterizer;

pub struct Desktop {
    imgui: imgui::Context,
    rasterizer: rasterizer::Rasterizer,
    last_rendering: Instant,
    background: background::Background,
    launch_bar: launch_bar::LaunchBar,
    pci_debug: pci_debug::PciDebug,
}

impl Desktop {
    pub async fn new(dimensions: [u32; 2]) -> Self {
        let mut rasterizer = rasterizer::Rasterizer::new(dimensions);
        let background = background::Background::new(&mut rasterizer);
        let launch_bar = launch_bar::LaunchBar::new(&mut rasterizer);
        let pci_debug = pci_debug::PciDebug::new(&mut rasterizer);

        let mut imgui = imgui::Context::create();
        // TODO: clipboard
        imgui.io_mut().display_framebuffer_scale = [1.0, 1.0];
        imgui.io_mut().display_size = [dimensions[0] as f32, dimensions[1] as f32];
        imgui.io_mut().font_global_scale = 1.0;
        imgui.io_mut().mouse_draw_cursor = true;
        imgui
            .fonts()
            .add_font(&[imgui::FontSource::DefaultFontData {
                config: Some(imgui::FontConfig {
                    size_pixels: 14.0,
                    ..Default::default()
                }),
            }]);

        imgui.set_platform_name(Some(imgui::ImString::from(format!("redshirt"))));
        imgui.set_renderer_name(Some(imgui::ImString::from(format!(
            "imgui-software-renderer"
        ))));
        let texture_id = rasterizer.add_texture(&imgui.fonts().build_rgba32_texture());
        imgui.fonts().tex_id = texture_id;

        Desktop {
            imgui,
            rasterizer,
            last_rendering: Instant::now(),
            background,
            launch_bar,
            pci_debug,
        }
    }

    /// Returns a buffer containing the RGB pixels.
    pub fn pixels(&self) -> &[u8] {
        self.rasterizer.pixels()
    }

    pub fn render(&mut self) {
        {
            let now = Instant::now();
            self.imgui.io_mut().delta_time = (now - self.last_rendering).as_secs_f32();
            self.last_rendering = now;
        }

        self.imgui.io_mut().mouse_pos = [300.0, 200.0]; // TODO:

        let ui = self.imgui.frame();
        self.background.draw(&ui);
        ui.show_demo_window(&mut true);
        ui.show_metrics_window(&mut true);
        ui.show_about_window(&mut true);
        self.pci_debug.draw(&ui);
        self.launch_bar.draw(&ui);

        let draw_data = ui.render();
        self.rasterizer.draw(&draw_data);
    }
}
