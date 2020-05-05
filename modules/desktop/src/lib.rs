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

mod rasterizer;

pub struct Desktop {
    imgui: imgui::Context,
    rasterizer: rasterizer::Rasterizer,
    last_rendering: Instant,
    background_texture_id: imgui::TextureId,
}

impl Desktop {
    pub fn new(dimensions: [u32; 2]) -> Self {
        let mut rasterizer = rasterizer::Rasterizer::new(dimensions);

        let background_texture_id = {
            let texture = image::load_from_memory(include_bytes!("../res/desktop-background.jpg"))
                .unwrap()
                .into_rgba();
            let width = texture.width();
            let height = texture.height();
            rasterizer.add_texture(&imgui::FontAtlasTexture {
                width,
                height,
                data: &texture.into_raw(),
            })
        };

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
            background_texture_id,
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

        let background_texture_id = self.background_texture_id;
        let ui = self.imgui.frame();
        let style = ui.push_style_vars(&[
            imgui::StyleVar::WindowPadding([0.0, 0.0]),
            imgui::StyleVar::WindowBorderSize(0.0),
        ]);
        imgui::Window::new(imgui::im_str!("background"))
            .opened(&mut true)
            .size([800.0, 600.0], imgui::Condition::FirstUseEver)
            .position([0.0, 0.0], imgui::Condition::FirstUseEver)
            .no_nav()
            .no_decoration()
            .no_inputs()
            .draw_background(false)
            .build(&ui, || {
                imgui::Image::new(background_texture_id, [800.0, 600.0]).build(&ui);
            });
        style.pop(&ui);
        ui.show_demo_window(&mut true);
        ui.show_metrics_window(&mut true);
        ui.show_about_window(&mut true);

        let draw_data = ui.render();
        self.rasterizer.draw(&draw_data);
    }
}
