// Copyright (C) 2019  Pierre Krieger
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

#![deny(intra_doc_link_resolution_failure)]

use futures::{channel::mpsc, channel::oneshot, prelude::*};
use parity_scale_codec::{DecodeAll, Encode as _};
use std::{sync::Arc, sync::Mutex, task::Context, task::Poll};

fn main() {
    let event_loop = winit::event_loop::EventLoop::with_user_event();
    // TODO: don't use channels, that's crap; use a state machine for async_main instead
    let (events_tx, events_rx) = mpsc::unbounded();
    let (win_open_tx, mut win_open_rx) = mpsc::unbounded();
    let mut async_main_future = Box::pin(async_main(events_rx, win_open_tx));

    let waker = {
        struct MyWaker(Mutex<winit::event_loop::EventLoopProxy<()>>);
        impl futures::task::ArcWake for MyWaker {
            fn wake_by_ref(arc_self: &Arc<Self>) {
                let _ = arc_self.0.lock().unwrap().send_event(());
            }
        }
        Arc::new(MyWaker(Mutex::new(event_loop.create_proxy())))
    };

    event_loop.run(move |event, window_creation, control_flow| {
        match event {
            winit::event::Event::UserEvent(()) => {}
            winit::event::Event::LoopDestroyed => return,
            ev => {
                let _ = events_tx.unbounded_send(ev);
            }
        }

        while let Ok(Some(rq)) = win_open_rx.try_next() {
            let result = winit::window::Window::new(&window_creation);
            let _ = rq.send(result);
        }

        match Future::poll(
            async_main_future.as_mut(),
            &mut Context::from_waker(&futures::task::waker(waker.clone())),
        ) {
            Poll::Ready(_) => *control_flow = winit::event_loop::ControlFlow::Exit,
            Poll::Pending => *control_flow = winit::event_loop::ControlFlow::Wait,
        }
    })
}

async fn async_main(
    mut events_rx: mpsc::UnboundedReceiver<winit::event::Event<()>>,
    win_open_rq: mpsc::UnboundedSender<
        oneshot::Sender<Result<winit::window::Window, winit::error::OsError>>,
    >,
) {
    let module = nametbd_core::module::Module::from_bytes(
        &include_bytes!("../../../modules/target/wasm32-wasi/release/vulkan-triangle.wasm")[..],
    )
    .unwrap();

    let mut system =
        nametbd_wasi_hosted::register_extrinsics(nametbd_core::system::SystemBuilder::new())
            // TODO: restore this .with_interface_handler(nametbd_time_interface::ffi::INTERFACE)
            .with_interface_handler(nametbd_stdout_interface::ffi::INTERFACE)
            .with_interface_handler(nametbd_tcp_interface::ffi::INTERFACE)
            .with_interface_handler(nametbd_vulkan_interface::INTERFACE)
            .with_interface_handler(nametbd_window_interface::ffi::INTERFACE)
            .with_startup_process(module)
            .build();

    let tcp = nametbd_tcp_hosted::TcpState::new();
    let mut vk = {
        #[link(name = "vulkan")]
        extern "system" {
            fn vkGetInstanceProcAddr(
                instance: usize,
                pName: *const u8,
            ) -> nametbd_vulkan_interface::PFN_vkVoidFunction;
        }
        nametbd_vulkan_interface::VulkanRedirect::new(vkGetInstanceProcAddr)
    };
    let mut windows = Vec::new();

    loop {
        let result = loop {
            let only_poll = match system.run() {
                nametbd_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                    pid,
                    thread_id,
                    extrinsic,
                    params,
                } => {
                    nametbd_wasi_hosted::handle_wasi(
                        &mut system,
                        extrinsic,
                        pid,
                        thread_id,
                        params,
                    );
                    true
                }
                nametbd_core::system::SystemRunOutcome::InterfaceMessage {
                    message_id,
                    interface,
                    message,
                } if interface == nametbd_tcp_interface::ffi::INTERFACE => {
                    let message: nametbd_tcp_interface::ffi::TcpMessage =
                        DecodeAll::decode_all(&message).unwrap();
                    tcp.handle_message(message_id, message).await;
                    continue;
                }
                nametbd_core::system::SystemRunOutcome::InterfaceMessage {
                    interface,
                    message,
                    ..
                } if interface == nametbd_stdout_interface::ffi::INTERFACE => {
                    let msg = nametbd_stdout_interface::ffi::StdoutMessage::decode_all(&message);
                    let nametbd_stdout_interface::ffi::StdoutMessage::Message(msg) = msg.unwrap();
                    print!("{}", msg);
                    continue;
                }
                /* TODO: restore this
                nametbd_core::system::SystemRunOutcome::InterfaceMessage {
                    message_id,
                    interface,
                    message,
                } if interface == nametbd_time_interface::ffi::INTERFACE => {
                    let answer = nametbd_time_hosted::time_message(&message);
                    system.answer_message(message_id.unwrap(), &answer);
                    continue;
                }*/
                nametbd_core::system::SystemRunOutcome::InterfaceMessage {
                    message_id,
                    interface,
                    message,
                } if interface == nametbd_vulkan_interface::INTERFACE => {
                    // TODO:
                    println!("received vk message: {:?}", message);
                    if let Some(response) = vk.handle(0, &message) {
                        // TODO: proper PID
                        system.answer_message(message_id.unwrap(), &response);
                    }
                    continue;
                }
                nametbd_core::system::SystemRunOutcome::InterfaceMessage {
                    message_id,
                    interface,
                    message,
                } if interface == nametbd_window_interface::ffi::INTERFACE => {
                    println!("received window message: {:?}", message);
                    let (tx, rx) = oneshot::channel();
                    win_open_rq.unbounded_send(tx).unwrap();
                    let window = rx.await.unwrap().unwrap();
                    windows.push(window);
                    system.answer_message(
                        message_id.unwrap(),
                        &nametbd_window_interface::ffi::WindowOpenResponse {
                            result: Ok(0), // TODO: correct ID
                        }
                        .encode(),
                    );
                    continue;
                }
                nametbd_core::system::SystemRunOutcome::Idle => false,
                other => break other,
            };

            while let Ok(Some(event)) = events_rx.try_next() {
                println!("windowing event: {:?}", event);
            }

            let event = if only_poll {
                match tcp.next_event().now_or_never() {
                    Some(e) => e,
                    None => continue,
                }
            } else {
                tcp.next_event().await
            };

            let (msg_to_respond, response_bytes) = match event {
                nametbd_tcp_hosted::TcpResponse::Accept(msg_id, msg) => (msg_id, msg.encode()),
                nametbd_tcp_hosted::TcpResponse::Listen(msg_id, msg) => (msg_id, msg.encode()),
                nametbd_tcp_hosted::TcpResponse::Open(msg_id, msg) => (msg_id, msg.encode()),
                nametbd_tcp_hosted::TcpResponse::Read(msg_id, msg) => (msg_id, msg.encode()),
                nametbd_tcp_hosted::TcpResponse::Write(msg_id, msg) => (msg_id, msg.encode()),
            };
            system.answer_message(msg_to_respond, &response_bytes);
        };

        match result {
            nametbd_core::system::SystemRunOutcome::ProgramFinished { pid, outcome } => {
                println!("Program finished {:?} => {:?}", pid, outcome);
            }
            _ => panic!(),
        }
    }
}
