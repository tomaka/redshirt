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

//! Implements the framebuffer interface by displaying each framebuffer in a window.
//!
//! # Usage
//!
//! - Create a [`FramebufferContext`]. This represents a collection of all the windows and
//! resources required to display the framebuffers.
//! - Create a [`FramebufferHandler`], passing a reference to the context. This type can be used
//! as a native process with the kernel.
//! - Call [`FramebufferContext::run`] for it to take control of your application and start
//! showing the framebuffers.
//!

use futures::{channel::mpsc, prelude::*};
use glium::glutin::event::{Event, WindowEvent};
use glium::glutin::event_loop::{ControlFlow, EventLoop, EventLoopProxy, EventLoopWindowTarget};
use parking_lot::Mutex;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_framebuffer_interface::ffi::INTERFACE;
use std::{
    collections::{hash_map::Entry, HashMap},
    convert::TryFrom as _,
    pin::Pin,
    sync::{atomic, Arc},
    task::{Context, Poll},
};

mod framebuffer;

/// Collection of all the resources required to display the framebuffers.
pub struct FramebufferContext {
    event_loop: EventLoop<()>,
    messages_tx: mpsc::UnboundedSender<HandlerToContext>,
    messages_rx: mpsc::UnboundedReceiver<HandlerToContext>,
}

/// Native program for `log` interface messages handling.
pub struct FramebufferHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    messages_tx: mpsc::UnboundedSender<HandlerToContext>,
}

/// Message from the handler to the context.
enum HandlerToContext {
    InterfaceMessage {
        emitter_pid: Pid,
        message: EncodedMessage,
    },
    ProcessDestroyed(Pid),
}

impl FramebufferContext {
    /// Creates a new context for the framebuffers.
    pub fn new() -> FramebufferContext {
        let event_loop = EventLoop::new();
        let (messages_tx, messages_rx) = mpsc::unbounded();

        FramebufferContext {
            event_loop,
            messages_tx,
            messages_rx,
        }
    }

    /// Runs the given future and processes the window's events loop.
    ///
    /// > **Note**: The idea behind this function is to take control of the entire application.
    /// >           In particular, the `Future` is expected to produce a value as a way to shut
    /// >           down the entire program.
    pub fn run<T: 'static>(self, future: impl Future<Output = T> + 'static) -> T {
        let proxy = self.event_loop.create_proxy();
        proxy.send_event(()).unwrap();

        // Creates an implementation of the `Stream` trait that produces events.
        enum LocalEvent<T> {
            FromHandler(HandlerToContext),
            FutureFinished(T),
        }
        let mut stream = Box::pin({
            let main_future = stream::once(async move { LocalEvent::FutureFinished(future.await) });

            let receiver_events = self.messages_rx.map(LocalEvent::FromHandler);

            stream::select(main_future, receiver_events)
        });

        // Active list of framebuffers.
        let mut framebuffers = HashMap::<(Pid, u32), framebuffer::Framebuffer>::new();

        self.event_loop
            .run(move |event, window_target, control_flow| {
                match event {
                    Event::RedrawRequested(window_id) => {
                        framebuffers
                            .values_mut()
                            .find(|fb| fb.window_id() == window_id)
                            .unwrap()
                            .draw();
                    }
                    Event::WindowEvent {
                        window_id: _,
                        event,
                    } => {
                        match event {
                            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                                // TODO: kill process?
                            }
                            _ => {}
                        }
                    }
                    Event::RedrawEventsCleared => {
                        // Waker that sends an event to the window when it is waken up.
                        let waker = {
                            struct Waker(Mutex<EventLoopProxy<()>>);
                            impl futures::task::ArcWake for Waker {
                                fn wake_by_ref(arc_self: &Arc<Self>) {
                                    let _ = arc_self.0.lock().send_event(());
                                }
                            }
                            futures::task::waker(Arc::new(Waker(Mutex::new(proxy.clone()))))
                        };
                        let mut context = Context::from_waker(&waker);

                        while let Poll::Ready(ev) = stream.poll_next_unpin(&mut context) {
                            let ev = match ev {
                                Some(LocalEvent::FromHandler(ev)) => ev,
                                Some(LocalEvent::FutureFinished(_)) | None => {
                                    *control_flow = ControlFlow::Exit;
                                    return;
                                }
                            };

                            match ev {
                                HandlerToContext::InterfaceMessage {
                                    emitter_pid,
                                    message,
                                } => {
                                    process_message(
                                        emitter_pid,
                                        message,
                                        &window_target,
                                        &mut framebuffers,
                                    );
                                }
                                HandlerToContext::ProcessDestroyed(pid) => {
                                    framebuffers.retain(|(p, _), _| *p != pid)
                                }
                            }
                        }
                    }
                    _ => {}
                }
            })
    }
}

fn process_message<T>(
    emitter_pid: Pid,
    message: EncodedMessage,
    window_target: &EventLoopWindowTarget<T>,
    framebuffers: &mut HashMap<(Pid, u32), framebuffer::Framebuffer>,
) {
    if message.0.len() < 1 {
        return;
    }

    match message.0[0] {
        0 => {
            // Create framebuffer message.
            if message.0.len() != 13 {
                return;
            }

            let fb_id = u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[1..5]).unwrap());
            let width = u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[5..9]).unwrap());
            let height = u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[9..13]).unwrap());
            if let Entry::Vacant(entry) = framebuffers.entry((emitter_pid, fb_id)) {
                let title = format!("redshirt - {:?} - framebuffer#{}", emitter_pid, fb_id);
                entry.insert(framebuffer::Framebuffer::new(
                    window_target,
                    &title,
                    width,
                    height,
                ));
            }
        }
        1 => {
            // Destroy framebuffer message.
            if message.0.len() != 5 {
                return;
            }

            let fb_id = u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[1..5]).unwrap());
            framebuffers.remove(&(emitter_pid, fb_id));
        }
        2 => {
            // Update framebuffer message.
            if message.0.len() < 5 {
                return;
            }

            let fb_id = u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[1..5]).unwrap());
            let framebuffer = match framebuffers.get_mut(&(emitter_pid, fb_id)) {
                Some(fb) => fb,
                None => return,
            };

            framebuffer.set_data(&message.0[5..]);
        }
        _ => {}
    }
}

impl FramebufferHandler {
    /// Initializes the state machine for framebuffer messages handling.
    ///
    /// The framebuffers will be rendered to the context passed as parameter.
    pub fn new(ctxt: &FramebufferContext) -> Self {
        FramebufferHandler {
            messages_tx: ctxt.messages_tx.clone(),
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
            .unbounded_send(HandlerToContext::InterfaceMessage {
                emitter_pid,
                message,
            })
            .unwrap();
    }

    fn process_destroyed(self, pid: Pid) {
        self.messages_tx
            .unbounded_send(HandlerToContext::ProcessDestroyed(pid))
            .unwrap();
    }

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}
