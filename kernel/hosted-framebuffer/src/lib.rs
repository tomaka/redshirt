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
use glium::glutin::event::{ElementState, Event, MouseButton, StartCause, WindowEvent};
use glium::glutin::event_loop::{ControlFlow, EventLoop, EventLoopProxy, EventLoopWindowTarget};
use parking_lot::Mutex;
use redshirt_core::native::{DummyMessageIdWrite, NativeProgramEvent, NativeProgramRef};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_framebuffer_interface::ffi;
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    convert::TryFrom as _,
    num::NonZeroU64,
    pin::Pin,
    sync::Arc,
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
    registered: atomic::Atomic<bool>,
    /// If `Some`, contains the registration ID towards the `interface` interface.
    registration_id: atomic::Atomic<Option<NonZeroU64>>,
    /// Number of message requests that need to be emitted.
    pending_message_requests: atomic::Atomic<u8>,
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
            registered: atomic::Atomic::new(false),
            registration_id: atomic::Atomic::new(None),
            pending_message_requests: atomic::Atomic::new(16),
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
                    message_id_write: Some(DummyMessageIdWrite),
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        ffi::INTERFACE_WITH_EVENTS,
                    )
                    .encode(),
                };
            }

            if let Some(registration_id) = self.registration_id.load(atomic::Ordering::Relaxed) {
                loop {
                    let v = self
                        .pending_message_requests
                        .load(atomic::Ordering::Relaxed);
                    if v == 0 {
                        break;
                    }
                    if self
                        .pending_message_requests
                        .compare_exchange(
                            v,
                            v - 1,
                            atomic::Ordering::Relaxed,
                            atomic::Ordering::Relaxed,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    return NativeProgramEvent::Emit {
                        interface: redshirt_interface_interface::ffi::INTERFACE,
                        message_id_write: Some(DummyMessageIdWrite),
                        message: redshirt_interface_interface::ffi::InterfaceMessage::NextMessage(
                            registration_id,
                        )
                        .encode(),
                    };
                }
            }

            loop {
                let mut lock = self.from_context.lock().await;
                // TODO: document the behaviour of this unwrap()
                match lock.next().await.unwrap() {
                    ContextToHandler::MessageAnswer { message_id, answer } => {
                        return NativeProgramEvent::Emit {
                            interface: redshirt_interface_interface::ffi::INTERFACE,
                            message_id_write: None,
                            message: redshirt_interface_interface::ffi::InterfaceMessage::Answer(
                                message_id,
                                answer.map(|m| m.0),
                            )
                            .encode(),
                        };
                    }
                }
            }
        })
    }

    fn message_response(self, _: MessageId, response: Result<EncodedMessage, ()>) {
        debug_assert!(self.registered.load(atomic::Ordering::Relaxed));

        // The first ever message response that can be received is the interface registration.
        if self
            .registration_id
            .load(atomic::Ordering::Relaxed)
            .is_none()
        {
            let registration_id =
                match redshirt_interface_interface::ffi::InterfaceRegisterResponse::decode(
                    response.unwrap(),
                )
                .unwrap()
                .result
                {
                    Ok(id) => id,
                    // A registration error means the interface has already been registered. Returning
                    // here stalls this state machine forever.
                    Err(_) => return,
                };

            self.registration_id
                .store(Some(registration_id), atomic::Ordering::Relaxed);
            return;
        }

        // If this is reached, the response is a response to a message request.
        self.pending_message_requests
            .fetch_add(1, atomic::Ordering::Relaxed);

        let notification =
            match redshirt_interface_interface::ffi::decode_notification(&response.unwrap().0)
                .unwrap()
            {
                redshirt_interface_interface::DecodedInterfaceOrDestroyed::Interface(n) => n,
                redshirt_interface_interface::DecodedInterfaceOrDestroyed::ProcessDestroyed(n) => {
                    self.to_context
                        .unbounded_send(HandlerToContext::ProcessDestroyed(n.pid))
                        .unwrap(); // TODO: document the behaviour of this unwrap()
                    return
                },
            };

        self.to_context
        .unbounded_send(HandlerToContext::InterfaceMessage {
            emitter_pid: notification.emitter_pid,
            message_id: notification.message_id,
            message: notification.actual_data,
        })
        .unwrap(); // TODO: document the behaviour of this unwrap()
    }
}

/// Turns a windowing event from the host format to the guest. `None` if there is no equivalent.
fn host_event_to_guest(ev: &WindowEvent) -> Option<EncodedMessage> {
    match ev {
        WindowEvent::KeyboardInput { input, .. } => {
            // TODO: is input.scancode the USB-conforming scancode?
            if let Ok(scancode) = u16::try_from(input.scancode) {
                let new_state = match input.state {
                    ElementState::Pressed => ffi::ElementState::Pressed,
                    ElementState::Released => ffi::ElementState::Released,
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
        WindowEvent::CursorLeft { .. } => {
            Some(ffi::Event::CursorMoved { new_position: None }.encode())
        }
        WindowEvent::CursorMoved { position, .. } => {
            assert!(position.x.is_normal());
            assert!(position.y.is_normal());
            let x = (position.x * 1000.0) as u64; // TODO: check conversion correctness?
            let y = (position.y * 1000.0) as u64; // TODO: check conversion correctness?
            Some(
                ffi::Event::CursorMoved {
                    new_position: Some((x, y)),
                }
                .encode(),
            )
        }
        WindowEvent::MouseInput { state, button, .. } => {
            let new_state = match state {
                ElementState::Pressed => ffi::ElementState::Pressed,
                ElementState::Released => ffi::ElementState::Released,
            };

            let button = match button {
                MouseButton::Left => Some(ffi::MouseButton::Main),
                MouseButton::Right => Some(ffi::MouseButton::Secondary),
                _ => None,
            };

            if let Some(button) = button {
                Some(ffi::Event::MouseButtonChange { button, new_state }.encode())
            } else {
                None
            }
        }
        _ => None,
    }
}
