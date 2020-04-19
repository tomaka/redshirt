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

//! Implements the framebuffer interface by showing the framebuffer in a window.

use futures::{channel::mpsc, prelude::*};
use glium::{
    program,
    texture::{ClientFormat, RawImage2d, Texture2d},
    uniform, Surface as _,
};
use parking_lot::Mutex;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_framebuffer_interface::ffi::INTERFACE;
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
    convert::TryFrom as _,
    pin::Pin,
    sync::{atomic, Arc},
    task::{Context, Poll},
    time::{Duration, Instant},
};

pub struct FramebufferContext {
    event_loop: glium::glutin::event_loop::EventLoop<()>,
    inner: FramebufferContextInner,
}

struct FramebufferContextInner {
    messages_tx: mpsc::UnboundedSender<(Pid, EncodedMessage)>,
    messages_rx: mpsc::UnboundedReceiver<(Pid, EncodedMessage)>,
    display: glium::Display,
    vertex_buffer: glium::VertexBuffer<Vertex>,
    index_buffer: glium::IndexBuffer<u16>,
    program: glium::Program,
    /// Active list of framebuffers.
    framebuffers: Mutex<HashMap<(Pid, u32), Framebuffer>>,
}

/// Native program for `log` interface messages handling.
pub struct FramebufferHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    messages_tx: mpsc::UnboundedSender<(Pid, EncodedMessage)>,
}

#[derive(Debug)]
struct Framebuffer {
    texture: Texture2d,
}

#[derive(Copy, Clone)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

glium::implement_vertex!(Vertex, position, tex_coords);

impl FramebufferContext {
    pub fn new() -> FramebufferContext {
        let event_loop = glium::glutin::event_loop::EventLoop::new();
        let display = {
            let wb = glium::glutin::window::WindowBuilder::new()
                .with_inner_size(glium::glutin::dpi::LogicalSize::new(640.0, 480.0))
                .with_title("redshirt");
            let cb = glium::glutin::ContextBuilder::new();
            glium::Display::new(wb, cb, &event_loop).unwrap()
        };

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

        let (messages_tx, messages_rx) = mpsc::unbounded();

        FramebufferContext {
            event_loop,
            inner: FramebufferContextInner {
                display,
                vertex_buffer,
                index_buffer,
                program,
                messages_tx,
                messages_rx,
                framebuffers: Mutex::new(HashMap::new()),
            },
        }
    }

    /// Runs the given future and processes the window's events loop.
    ///
    /// > **Note**: The idea behind this function is to take control of the entire application.
    /// >           In particular, the `Future` is expected to produce a value as a way to shut
    /// >           down the entire program.
    pub fn run<T>(self, future: impl Future<Output = T> + 'static) -> T {
        let mut inner = self.inner;
        let proxy = self.event_loop.create_proxy();

        let mut future = Box::pin(future);

        self.event_loop.run(move |event, _, control_flow| {
            let run_callback = match event.to_static() {
                Some(glium::glutin::event::Event::NewEvents(cause)) => match cause {
                    glium::glutin::event::StartCause::ResumeTimeReached { .. }
                    | glium::glutin::event::StartCause::Init => true,
                    _ => false,
                },
                Some(event) => {
                    //events_buffer.push(event);
                    false
                }
                None => {
                    // Ignore this event.
                    false
                }
            };

            inner.process_messages();
            inner.draw();

            let next_frame_time = Instant::now() + Duration::from_nanos(16666667);
            *control_flow = glium::glutin::event_loop::ControlFlow::WaitUntil(next_frame_time);

            // Polling the future to make progress.
            {
                struct Waker(Mutex<glium::glutin::event_loop::EventLoopProxy<()>>);
                impl futures::task::ArcWake for Waker {
                    fn wake_by_ref(arc_self: &Arc<Self>) {
                        let _ = arc_self.0.lock().send_event(());
                    }
                }
                let waker = futures::task::waker(Arc::new(Waker(Mutex::new(proxy.clone()))));
                match future.poll_unpin(&mut Context::from_waker(&waker)) {
                    Poll::Ready(val) => unimplemented!(), // TODO:
                    Poll::Pending => {}
                }
            }
        })
    }
}

impl FramebufferContextInner {
    fn draw(&mut self) {
        let mut target = self.display.draw();
        target.clear_color(0.0, 0.0, 0.0, 0.0);
        if let Some(fb) = self.framebuffers.lock().values().next() {
            let uniforms = uniform! {
                tex: &fb.texture,
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
        }
        target.finish().unwrap();
    }

    fn process_messages(&mut self) {
        while let Some(Some((emitter_pid, message))) = self.messages_rx.next().now_or_never() {
            if message.0.len() < 1 {
                continue;
            }

            match message.0[0] {
                0 => {
                    // Create framebuffer message.
                    if message.0.len() != 13 {
                        continue;
                    }

                    let fb_id = u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[1..5]).unwrap());
                    let width = u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[5..9]).unwrap());
                    let height =
                        u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[9..13]).unwrap());
                    if let Entry::Vacant(entry) =
                        self.framebuffers.lock().entry((emitter_pid, fb_id))
                    {
                        entry.insert(Framebuffer {
                            texture: Texture2d::empty(&self.display, width, height).unwrap(),
                        });
                    }
                }
                1 => {
                    // Destroy framebuffer message.
                    if message.0.len() != 5 {
                        continue;
                    }

                    let fb_id = u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[1..5]).unwrap());
                    self.framebuffers.lock().remove(&(emitter_pid, fb_id));
                }
                2 => {
                    // Update framebuffer message.
                    if message.0.len() < 5 {
                        continue;
                    }

                    let fb_id = u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[1..5]).unwrap());
                    let framebuffers = self.framebuffers.lock();
                    let framebuffer = match framebuffers.get(&(emitter_pid, fb_id)) {
                        Some(fb) => fb,
                        None => continue,
                    };

                    if u32::try_from(message.0.len())
                        != Ok(5u32.saturating_add(
                            3u32.saturating_mul(framebuffer.texture.width())
                                .saturating_mul(framebuffer.texture.height()),
                        ))
                    {
                        continue;
                    }

                    let rect = glium::Rect {
                        left: 0,
                        bottom: 0,
                        width: framebuffer.texture.width(),
                        height: framebuffer.texture.height(),
                    };

                    framebuffer.texture.write(
                        rect,
                        RawImage2d {
                            data: Cow::Borrowed(&message.0[5..]),
                            width: framebuffer.texture.width(),
                            height: framebuffer.texture.height(),
                            format: ClientFormat::U8U8U8,
                        },
                    );
                }
                _ => {}
            }
        }
    }
}

impl FramebufferHandler {
    /// Initializes the state machine for framebuffer messages handling.
    ///
    /// The framebuffers will be rendered to the context passed as parameter.
    pub fn new(ctxt: &FramebufferContext) -> Self {
        FramebufferHandler {
            messages_tx: ctxt.inner.messages_tx.clone(),
            registered: atomic::AtomicBool::new(false),
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a FramebufferHandler {
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        Box::pin(async move {
            if !self.registered.swap(true, atomic::Ordering::Relaxed) {
                return NativeProgramEvent::Emit {
                    interface: redshirt_interface_interface::ffi::INTERFACE,
                    message_id_write: None,
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        INTERFACE,
                    )
                    .encode(),
                };
            }

            loop {
                futures::pending!()
            }
        })
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        _: Option<MessageId>,
        emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        debug_assert_eq!(interface, INTERFACE);
        self.messages_tx
            .unbounded_send((emitter_pid, message))
            .unwrap();
    }

    fn process_destroyed(self, pid: Pid) {
        // TODO: inform the context
    }

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}
