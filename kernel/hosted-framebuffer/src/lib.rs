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

use futures::{channel::mpsc, lock::Mutex as FutureMutex, prelude::*};
use glium::glutin::event::{ElementState, Event, StartCause, WindowEvent};
use glium::glutin::event_loop::{ControlFlow, EventLoop, EventLoopProxy, EventLoopWindowTarget};
use parking_lot::Mutex;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_framebuffer_interface::ffi;
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    convert::TryFrom as _,
    pin::Pin,
    sync::{atomic, Arc},
    task::{Context, Poll},
};

mod framebuffer;

/// Collection of all the resources required to display the framebuffers.
pub struct FramebufferContext {
    event_loop: EventLoop<()>,
    to_context: mpsc::UnboundedSender<HandlerToContext>,
    from_handler: mpsc::UnboundedReceiver<HandlerToContext>,
    to_handlers: Mutex<Vec<mpsc::UnboundedSender<ContextToHandler>>>,
}

/// Native program for `log` interface messages handling.
pub struct FramebufferHandler {
    /// If true, we have sent the interface registration message.
    registered: atomic::AtomicBool,
    to_context: mpsc::UnboundedSender<HandlerToContext>,
    from_context: FutureMutex<mpsc::UnboundedReceiver<ContextToHandler>>,
}

/// Message from the handler to the context.
enum HandlerToContext {
    InterfaceMessage {
        emitter_pid: Pid,
        message_id: Option<MessageId>,
        message: EncodedMessage,
    },
    ProcessDestroyed(Pid),
}

/// Message from the context to the handler.
enum ContextToHandler {
    /// A message answer is ready.
    MessageAnswer {
        message_id: MessageId,
        answer: Result<EncodedMessage, ()>,
    },
}

impl FramebufferContext {
    /// Creates a new context for the framebuffers.
    pub fn new() -> FramebufferContext {
        let event_loop = EventLoop::new();
        let (to_context, from_handler) = mpsc::unbounded();

        FramebufferContext {
            event_loop,
            to_context,
            from_handler,
            to_handlers: Mutex::new(Vec::new()),
        }
    }

    /// Runs the given future and processes the window's events loop.
    ///
    /// > **Note**: The idea behind this function is to take control of the entire application.
    /// >           In particular, the `Future` is expected to produce a value as a way to shut
    /// >           down the entire program.
    pub fn run<T: 'static>(self, future: impl Future<Output = T> + 'static) -> T {
        // Futures waker that sends an event to the window when it is waken up.
        let waker = {
            let proxy = self.event_loop.create_proxy();

            struct Waker(Mutex<EventLoopProxy<()>>);
            impl futures::task::ArcWake for Waker {
                fn wake_by_ref(arc_self: &Arc<Self>) {
                    let _ = arc_self.0.lock().send_event(());
                }
            }

            futures::task::waker(Arc::new(Waker(Mutex::new(proxy))))
        };

        // Creates an implementation of the `Stream` trait that produces events.
        enum LocalEvent<T> {
            FromHandler(HandlerToContext),
            FutureFinished(T),
        }
        let mut stream = Box::pin({
            let main_future = stream::once(async move { LocalEvent::FutureFinished(future.await) });
            let receiver_events = self.from_handler.map(LocalEvent::FromHandler);
            stream::select(main_future, receiver_events)
        });

        // Active list of framebuffers.
        let mut framebuffers =
            HashMap::<(Pid, u32), (framebuffer::Framebuffer, VecDeque<MessageId>)>::new();

        // How to send messages to all the handlers.
        let mut to_handlers = self.to_handlers.into_inner();

        self.event_loop
            .run(move |event, window_target, control_flow| {
                match event {
                    Event::RedrawRequested(window_id) => {
                        framebuffers
                            .values_mut()
                            .find(|(fb, _)| fb.window_id() == window_id)
                            .unwrap()
                            .0
                            .draw();
                    }
                    Event::WindowEvent { window_id, event } => {
                        if let Some(guest_event) = host_event_to_guest(&event) {
                            let framebuffer = framebuffers
                                .values_mut()
                                .find(|(fb, _)| fb.window_id() == window_id)
                                .unwrap();
                            if let Some(message_id) = framebuffer.1.pop_front() {
                                to_handlers.retain(|sender| {
                                    let msg = ContextToHandler::MessageAnswer {
                                        message_id,
                                        answer: Ok(guest_event.clone()),
                                    };
                                    sender.unbounded_send(msg).is_ok()
                                });
                            } else {
                                // TODO: log something?
                            }
                        }

                        match event {
                            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                                // TODO: kill process?
                            }
                            _ => {}
                        }
                    }
                    Event::RedrawEventsCleared => {
                        // The control flow is always set to `Wait`. What we want to achieve is
                        // wake up the events loop whenever the stream is ready by sending a
                        // dummy event to it.
                        *control_flow = ControlFlow::Wait;
                    }
                    Event::NewEvents(StartCause::Init) | Event::UserEvent(()) => {
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
                                    message_id,
                                    message,
                                } => {
                                    process_message(
                                        emitter_pid,
                                        message_id,
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
    message_id: Option<MessageId>,
    message: EncodedMessage,
    window_target: &EventLoopWindowTarget<T>,
    framebuffers: &mut HashMap<(Pid, u32), (framebuffer::Framebuffer, VecDeque<MessageId>)>,
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
                entry.insert((
                    framebuffer::Framebuffer::new(window_target, &title, width, height),
                    VecDeque::new(),
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
                Some(fb) => &mut fb.0,
                None => return,
            };

            framebuffer.set_data(&message.0[5..]);
        }
        3 => {
            if let Some(message_id) = message_id {
                // Ask for the next input event message.
                if message.0.len() != 5 {
                    return;
                }

                let fb_id = u32::from_le_bytes(<[u8; 4]>::try_from(&message.0[1..5]).unwrap());
                if let Some((_, messages)) = framebuffers.get_mut(&(emitter_pid, fb_id)) {
                    messages.push_back(message_id);
                }
            }
        }
        _ => {} // TODO:
    }
}

impl FramebufferHandler {
    /// Initializes the state machine for framebuffer messages handling.
    ///
    /// The framebuffers will be rendered to the context passed as parameter.
    pub fn new(ctxt: &FramebufferContext) -> Self {
        let (to_handler, from_context) = mpsc::unbounded();

        ctxt.to_handlers.lock().push(to_handler);

        FramebufferHandler {
            to_context: ctxt.to_context.clone(),
            from_context: FutureMutex::new(from_context),
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
                        ffi::INTERFACE_WITH_EVENTS,
                    )
                    .encode(),
                };
            }

            loop {
                let mut lock = self.from_context.lock().await;
                // TODO: document the behaviour of this unwrap()
                match lock.next().await.unwrap() {
                    ContextToHandler::MessageAnswer { message_id, answer } => {
                        return NativeProgramEvent::Answer { message_id, answer };
                    }
                }
            }
        })
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        debug_assert_eq!(interface, ffi::INTERFACE_WITH_EVENTS);
        self.to_context
            .unbounded_send(HandlerToContext::InterfaceMessage {
                emitter_pid,
                message_id,
                message,
            })
            .unwrap(); // TODO: document the behaviour of this unwrap()
    }

    fn process_destroyed(self, pid: Pid) {
        self.to_context
            .unbounded_send(HandlerToContext::ProcessDestroyed(pid))
            .unwrap(); // TODO: document the behaviour of this unwrap()
    }

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}

/// Turns a windowing event from the host format to the guest. `None` if there is no equivalent.
fn host_event_to_guest(ev: &WindowEvent) -> Option<EncodedMessage> {
    match ev {
        WindowEvent::KeyboardInput { input, .. } => {
            if let Ok(scancode) = u16::try_from(input.scancode) {
                let new_state = match input.state {
                    ElementState::Pressed => ffi::Keystate::Pressed,
                    ElementState::Released => ffi::Keystate::Released,
                };

                Some(
                    ffi::Event::KeyboardChange {
                        scancode,
                        new_state,
                    }
                    .encode(),
                )
            } else {
                None
            }
        }
        _ => None,
    }
}
