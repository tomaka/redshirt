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

/// Launch bar widget.
pub struct LaunchBar {
    background_texture_id: imgui::TextureId,
}

impl LaunchBar {
    /// Registers resources towards the rasterizer.
    pub fn new(rasterizer: &mut Rasterizer) -> Self {
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

        LaunchBar {
            background_texture_id,
        }
    }

    /// Draws the widget on the UI.
    ///
    /// Must then be rendered with the rasterized that was passed at initialization.
    pub fn draw(&mut self, ui: &imgui::Ui) {
        let style_override = ui.push_style_vars(&[
            imgui::StyleVar::WindowPadding([0.0, 0.0]),
            imgui::StyleVar::WindowBorderSize(0.0),
        ]);

        imgui::Window::new(imgui::im_str!("launch-bar"))
            .opened(&mut true)
            .size([400.0, 50.0], imgui::Condition::FirstUseEver)
            .position([400.0, 600.0], imgui::Condition::FirstUseEver)
            .position_pivot([0.5, 1.0])
            .no_nav()
            .title_bar(false)
            .resizable(false)
            .movable(false)
            .collapsible(false)
            .menu_bar(false)
            .bg_alpha(0.5)
            .build(&ui, || {
                //imgui::Image::new(self.background_texture_id, [800.0, 600.0]).build(&ui);
                ui.columns(10, imgui::im_str!("launch-bar-icons"), true);
                for _ in 0..10 {
                    imgui::Image::new(self.background_texture_id, [800.0, 600.0]).build(&ui);
                    ui.next_column();
                }
            });

        style_override.pop(&ui);
    }
}
