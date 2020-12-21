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

/// Background image widget.
pub struct Background {
    background_texture_id: imgui::TextureId,
}

impl Background {
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

        Background {
            background_texture_id,
        }
    }

    /// Draws the widget on the UI.
    ///
    /// Must then be rendered with the rasterized that was passed at initialization.
    pub fn draw(&mut self, ui: &imgui::Ui) {
        let style = ui.push_style_vars(&[
            imgui::StyleVar::WindowPadding([0.0, 0.0]),
            imgui::StyleVar::WindowBorderSize(0.0),
        ]);
        let size = [800.0, 600.0]; // TODO: dammit, I have no idea how to retreive this from the Ui
        imgui::Window::new(imgui::im_str!("background"))
            .opened(&mut true)
            .size(size, imgui::Condition::Always)
            .position([0.0, 0.0], imgui::Condition::FirstUseEver)
            .no_nav()
            .no_decoration()
            .no_inputs()
            .draw_background(false)
            .build(&ui, || {
                imgui::ImageButton::new(self.background_texture_id, size).build(&ui);
            });
        style.pop(&ui);
    }
}
