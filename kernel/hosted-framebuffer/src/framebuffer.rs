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

use glium::{
    glutin::window::WindowId,
    program,
    texture::{ClientFormat, RawImage2d, Texture2d},
    uniform, Surface as _,
};
use std::{borrow::Cow, convert::TryFrom as _};

/// Window and framebuffer. Implicitely associated to the event loop that has been passed when
/// creating it.
pub struct Framebuffer {
    window_id: WindowId,
    display: glium::Display,
    vertex_buffer: glium::VertexBuffer<Vertex>,
    index_buffer: glium::IndexBuffer<u16>,
    program: glium::Program,
    texture: Texture2d,
}

impl Framebuffer {
    /// Creates a new window for displaying a framebuffer.
    // TODO: check if zero dimensions
    // TODO: return Result
    pub fn new<T>(
        event_loop: &glium::glutin::event_loop::EventLoopWindowTarget<T>,
        title: &str,
        width: u32,
        height: u32,
    ) -> Framebuffer {
        let wb = glium::glutin::window::WindowBuilder::new()
            .with_inner_size(glium::glutin::dpi::LogicalSize::new(
                width as f64,
                height as f64,
            ))
            .with_resizable(false)
            // Windows start invisible and become visible when the data is set for the first time.
            .with_visible(false)
            .with_title(title);

        let ctxt = glium::glutin::ContextBuilder::new()
            .build_windowed(wb, event_loop)
            .unwrap();

        let window_id = ctxt.window().id();
        let display = glium::Display::from_gl_window(ctxt).unwrap();

        let vertex_buffer = {
            glium::VertexBuffer::new(
                &display,
                &[
                    // Since the framebuffer interface sends texture data from top to bottom,
                    // we accept having an upside down texture and invert the image.
                    Vertex {
                        position: [-1.0, 1.0],
                        tex_coords: [0.0, 0.0],
                    },
                    Vertex {
                        position: [-1.0, -1.0],
                        tex_coords: [0.0, 1.0],
                    },
                    Vertex {
                        position: [1.0, -1.0],
                        tex_coords: [1.0, 1.0],
                    },
                    Vertex {
                        position: [1.0, 1.0],
                        tex_coords: [1.0, 0.0],
                    },
                ],
            )
            .unwrap()
        };

        let index_buffer = glium::IndexBuffer::new(
            &display,
            glium::index::PrimitiveType::TriangleStrip,
            &[1 as u16, 2, 0, 3],
        )
        .unwrap();

        let program = program!(&display,
            140 => {
                vertex: "
                    #version 140
                    in vec2 position;
                    in vec2 tex_coords;
                    out vec2 v_tex_coords;
                    void main() {
                        gl_Position = vec4(position, 0.0, 1.0);
                        v_tex_coords = tex_coords;
                    }
                ",

                fragment: "
                    #version 140
                    uniform sampler2D tex;
                    in vec2 v_tex_coords;
                    out vec4 f_color;
                    void main() {
                        f_color = texture(tex, v_tex_coords);
                    }
                "
            },
        )
        .unwrap();

        let texture = Texture2d::empty(&display, width, height).unwrap();

        Framebuffer {
            window_id,
            display,
            vertex_buffer,
            index_buffer,
            program,
            texture,
        }
    }

    /// Returns the [`WindowId`] of this window.
    ///
    /// Makes it possible to know if an event received on the events loop corresponds to this
    /// instance.
    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// Sets the content of this framebuffer and dispatches a window redraw request on the window.
    pub fn set_data(&mut self, data: &[u8]) {
        if u32::try_from(data.len())
            != Ok(3u32
                .saturating_mul(self.texture.width())
                .saturating_mul(self.texture.height()))
        {
            // TODO: log this?
            return;
        }

        let rect = glium::Rect {
            left: 0,
            bottom: 0,
            width: self.texture.width(),
            height: self.texture.height(),
        };

        self.texture.write(
            rect,
            RawImage2d {
                data: Cow::Borrowed(data),
                width: self.texture.width(),
                height: self.texture.height(),
                format: ClientFormat::U8U8U8,
            },
        );

        let window = self.display.gl_window();
        let window = window.window();
        window.set_visible(true);
        window.request_redraw();
    }

    /// Refreshes the framebuffer.
    pub fn draw(&mut self) {
        let mut target = self.display.draw();
        target.clear_color(0.0, 0.0, 0.0, 0.0);

        let uniforms = uniform! {
            tex: &self.texture,
        };

        target
            .draw(
                &self.vertex_buffer,
                &self.index_buffer,
                &self.program,
                &uniforms,
                &Default::default(),
            )
            .unwrap();

        target.finish().unwrap();
    }
}

#[derive(Copy, Clone)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

glium::implement_vertex!(Vertex, position, tex_coords);
