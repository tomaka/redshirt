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

use cgmath::{Vector2, Vector4};
use imgui::internal::RawWrapper as _;

pub struct Rasterizer {
    surface: Vec<u8>,
    framebuffer_dimensions: [u32; 2],
    framebuffer_dimensions_f32: [f32; 2],
    textures: Vec<Texture>,
}

struct Texture {
    dimensions_px: Vector2<f32>,
    width: i32,
    data: Vec<u8>,
}

impl Rasterizer {
    pub fn new(dimensions: [u32; 2]) -> Self {
        Rasterizer {
            // TODO: use proper try stuff and checked mul and all
            surface: vec![0; (dimensions[0] * dimensions[1] * 3) as usize],
            framebuffer_dimensions: dimensions,
            framebuffer_dimensions_f32: [dimensions[0] as f32, dimensions[1] as f32],
            textures: Vec::new(),
        }
    }

    /// Returns a buffer containing the RGB pixels.
    pub fn pixels(&self) -> &[u8] {
        &self.surface
    }

    /// Adds an RGBA texture and returns a `TextureId` later passed by imgui when drawing.
    pub fn add_texture(&mut self, texture: &imgui::FontAtlasTexture) -> imgui::TextureId {
        let new_id = From::from(self.textures.len());
        self.textures.push(Texture {
            dimensions_px: Vector2::new(texture.width as f32, texture.height as f32),
            data: texture.data.to_owned(),
            width: texture.width as i32,
        });
        new_id
    }

    /// Draws the given data from imgui.
    ///
    /// # Panic
    ///
    /// Can panic if uses textures whose ID hasn't earlier been returned by
    /// [`Rasterizer::add_texture`].
    ///
    pub fn draw(&mut self, draw_data: &imgui::DrawData) {
        for draw_list in draw_data.draw_lists() {
            self.draw_list(&draw_list);
        }
    }

    fn draw_list(&mut self, draw_list: &imgui::DrawList) {
        let mut index_start = 0;

        for cmd in draw_list.commands() {
            match cmd {
                imgui::DrawCmd::Elements {
                    count,
                    cmd_params:
                        imgui::DrawCmdParams {
                            clip_rect,
                            texture_id,
                            ..
                        },
                } => {
                    let index_range = {
                        let start = index_start;
                        index_start += count;
                        start..index_start
                    };

                    for triangle in draw_list.idx_buffer()[index_range].chunks(3) {
                        let vertices = [
                            draw_list.vtx_buffer()[usize::from(triangle[0])],
                            draw_list.vtx_buffer()[usize::from(triangle[1])],
                            draw_list.vtx_buffer()[usize::from(triangle[2])],
                        ];

                        self.draw_triangle(vertices, clip_rect, texture_id);
                    }
                }
                imgui::DrawCmd::ResetRenderState => (),
                imgui::DrawCmd::RawCallback { callback, raw_cmd } => unsafe {
                    callback(draw_list.raw(), raw_cmd)
                },
            }
        }
    }

    /// Draws a single triange made from the three given vertices on the surface.
    fn draw_triangle(
        &mut self,
        vertices: [imgui::DrawVert; 3],
        clip_rect: [f32; 4],
        texture_id: imgui::TextureId,
    ) {
        // Turn the vertices into floating points.
        let screen_coords = [
            Vector2::new(vertices[0].pos[0], vertices[0].pos[1]),
            Vector2::new(vertices[1].pos[0], vertices[1].pos[1]),
            Vector2::new(vertices[2].pos[0], vertices[2].pos[1]),
        ];

        // Turn the UV coordinates into floating points.
        let uv_coords = [
            Vector2::new(vertices[0].uv[0], vertices[0].uv[1]),
            Vector2::new(vertices[1].uv[0], vertices[1].uv[1]),
            Vector2::new(vertices[2].uv[0], vertices[2].uv[1]),
        ];

        // Turn the colors into floating points.
        let colors = [
            Vector4::new(
                vertices[0].col[0] as f32 / 255.0,
                vertices[0].col[1] as f32 / 255.0,
                vertices[0].col[2] as f32 / 255.0,
                vertices[0].col[3] as f32 / 255.0,
            ),
            Vector4::new(
                vertices[1].col[0] as f32 / 255.0,
                vertices[1].col[1] as f32 / 255.0,
                vertices[1].col[2] as f32 / 255.0,
                vertices[1].col[3] as f32 / 255.0,
            ),
            Vector4::new(
                vertices[2].col[0] as f32 / 255.0,
                vertices[2].col[1] as f32 / 255.0,
                vertices[2].col[2] as f32 / 255.0,
                vertices[2].col[3] as f32 / 255.0,
            ),
        ];

        // Slope from vertices 2 to 0 and 1 to 0.
        let screen_coords_slope = [
            screen_coords[1] - screen_coords[0],
            screen_coords[2] - screen_coords[0],
        ];
        let denominator = cross(screen_coords_slope[0], screen_coords_slope[1]);
        let uv_slope = [uv_coords[1] - uv_coords[0], uv_coords[2] - uv_coords[0]];
        let colors_slope = [colors[1] - colors[0], colors[2] - colors[0]];

        // Then determine the bounding box of our triangle, this time in integral pixels.
        let min_x = screen_coords[0]
            .x
            .min(screen_coords[1].x)
            .min(screen_coords[2].x)
            .floor();
        let max_x = screen_coords[0]
            .x
            .max(screen_coords[1].x)
            .max(screen_coords[2].x)
            .ceil();
        let min_y = screen_coords[0]
            .y
            .min(screen_coords[1].y)
            .min(screen_coords[2].y)
            .floor();
        let max_y = screen_coords[0]
            .y
            .max(screen_coords[1].y)
            .max(screen_coords[2].y)
            .ceil();

        // Adjust these values for the clip rectangle and framebuffer dimensions.
        let min_x = 0.0f32.max(clip_rect[0].max(min_x)) as i32;
        let max_x = self.framebuffer_dimensions_f32[0].min(clip_rect[2].min(max_x)) as i32;
        let min_y = 0.0f32.max(clip_rect[1].max(min_y)) as i32;
        let max_y = self.framebuffer_dimensions_f32[1].min(clip_rect[3].min(max_y)) as i32;

        // Now iterate over each pixel within the bounding box and determine whether it is
        // inside the triangle.
        for y in min_y..max_y {
            for x in min_x..max_x {
                let float_coords = Vector2::new(x as f32 + 0.5, y as f32 + 0.5);
                let coords_rel_v0 = float_coords - screen_coords[0];

                let barycentric_coords = Vector2::new(
                    cross(coords_rel_v0, screen_coords_slope[1]) / denominator,
                    cross(screen_coords_slope[0], coords_rel_v0) / denominator,
                );

                // Check whether we are inside the triangle.
                // TODO: do some MSAA here?
                if barycentric_coords.x < 0.0
                    || barycentric_coords.y < 0.0
                    || (barycentric_coords.x + barycentric_coords.y) >= 1.0
                {
                    continue;
                }

                let color = colors[0]
                    + colors_slope[0] * barycentric_coords.x
                    + colors_slope[1] * barycentric_coords.y;
                let uv = uv_coords[0]
                    + uv_slope[0] * barycentric_coords.x
                    + uv_slope[1] * barycentric_coords.y;
                let texture_sample = self.texture_sample(texture_id, uv);
                let actual_color = Vector4::new(
                    texture_sample.x * color.x,
                    texture_sample.y * color.y,
                    texture_sample.z * color.z,
                    texture_sample.w * color.w,
                );
                self.put_pixel(Vector2::new(x, y), actual_color);
            }
        }
    }

    /// Returns the value of a texture at the given UV coords.
    fn texture_sample(
        &self,
        texture_id: imgui::TextureId,
        uv_coords: Vector2<f32>,
    ) -> Vector4<f32> {
        let texture = &self.textures[texture_id.id()];

        let uv_pixels = Vector2::new(
            uv_coords.x * texture.dimensions_px.x,
            uv_coords.y * texture.dimensions_px.y,
        );

        // Alright. Pixels are normally defined by their middle. For example, the top-left pixel
        // of the texture has coordinates `(0.5, 0.5)`. If `uv_coords` is `(0.5, 0.5)` we want to
        // get the exact value of the top-left-most pixel. If `uv_coords` is for example
        // `(1.0, 1.0)`, we want to get one quarter the top-left pixel, and one quarter the pixels
        // at rows 1,2, 2,1 and 2,2.
        // In order to make calculations easier, we shift all this machinery and defined pixels as
        // their top-left position by subtracting `(0.5, 0.5)` to the UV coords.
        // TODO: this probably means that we get out of bounds at the bottom-right corner; fix this
        let uv_pixels_hack = uv_pixels - Vector2::new(0.5, 0.5);

        let adjacent_pixels = [
            (
                Vector2::new(
                    uv_pixels_hack.x.floor() as i32,
                    uv_pixels_hack.y.floor() as i32,
                ),
                uv_pixels_hack.x.fract() * uv_pixels_hack.y.fract(),
            ),
            (
                Vector2::new(
                    uv_pixels_hack.x.floor() as i32,
                    uv_pixels_hack.y.ceil() as i32,
                ),
                uv_pixels_hack.x.fract() * (1.0 - uv_pixels_hack.y.fract()),
            ),
            (
                Vector2::new(
                    uv_pixels_hack.x.ceil() as i32,
                    uv_pixels_hack.y.floor() as i32,
                ),
                (1.0 - uv_pixels_hack.x.fract()) * uv_pixels_hack.y.fract(),
            ),
            (
                Vector2::new(
                    uv_pixels_hack.x.ceil() as i32,
                    uv_pixels_hack.y.ceil() as i32,
                ),
                (1.0 - uv_pixels_hack.x.fract()) * (1.0 - uv_pixels_hack.y.fract()),
            ),
        ];

        let mut total = Vector4::new(0.0, 0.0, 0.0, 0.0);
        for (coords, weight) in &adjacent_pixels {
            let tex_slice = {
                let idx = 4 * (coords.x + coords.y * texture.width) as usize;
                &texture.data[idx..idx + 4]
            };
            total += *weight
                * Vector4::new(
                    tex_slice[0] as f32,
                    tex_slice[1] as f32,
                    tex_slice[2] as f32,
                    tex_slice[3] as f32,
                )
                / 255.0;
        }
        total
    }

    /// Writes a single pixel with the given color at the given coordinates.
    ///
    /// Applies alpha blending using the fourth color component as the alpha value.
    fn put_pixel(&mut self, coords: Vector2<i32>, color: Vector4<f32>) {
        let rgb_out = {
            // TODO: proper try_from
            let index =
                ((coords.x as u32 + coords.y as u32 * self.framebuffer_dimensions[0]) * 3) as usize;
            &mut self.surface[index..index + 3]
        };

        if color.w >= 1.0 {
            rgb_out[0] = (color.x * 255.0).round() as u8;
            rgb_out[1] = (color.y * 255.0).round() as u8;
            rgb_out[2] = (color.z * 255.0).round() as u8;
        } else {
            let one_minus_alpha = 1.0 - color.w;
            rgb_out[0] = (rgb_out[0] as f32 * one_minus_alpha) as u8
                + (color.x * 255.0 * color.w).round() as u8;
            rgb_out[1] = (rgb_out[0] as f32 * one_minus_alpha) as u8
                + (color.y * 255.0 * color.w).round() as u8;
            rgb_out[2] = (rgb_out[0] as f32 * one_minus_alpha) as u8
                + (color.z * 255.0 * color.w).round() as u8;
        }
    }
}

fn cross(v1: Vector2<f32>, v2: Vector2<f32>) -> f32 {
    v1.x * v2.y - v1.y * v2.x
}
