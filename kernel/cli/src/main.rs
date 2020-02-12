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

#![deny(intra_doc_link_resolution_failure)]

use parking_lot::Mutex;
use std::{fs, future::Future, path::PathBuf, process, sync::Arc, task::Poll};
use structopt::StructOpt;
use winit::event_loop::EventLoop;

#[derive(Debug, StructOpt)]
#[structopt(name = "redshirt-cli", about = "Redshirt modules executor.")]
struct CliOptions {
    /// WASM file to run.
    #[structopt(parse(from_os_str))]
    wasm_file: PathBuf,
}

fn main() {
    futures::executor::block_on(async_main());
}

async fn async_main() {
    let event_loop = EventLoop::with_user_event();

    let cli_requested_process = {
        let cli_opts = CliOptions::from_args();
        let wasm_file_content = fs::read(cli_opts.wasm_file).expect("failed to read input file");
        redshirt_core::module::Module::from_bytes(&wasm_file_content)
            .expect("failed to parse input file")
    };

    let window = winit::window::Window::new(&event_loop).unwrap();

    let mut system = redshirt_core::system::SystemBuilder::new()
        .with_native_program(redshirt_time_hosted::TimerHandler::new())
        .with_native_program(redshirt_tcp_hosted::TcpHandler::new())
        .with_native_program(redshirt_log_hosted::LogHandler::new())
        .with_native_program(redshirt_webgpu_hosted::WebGPUHandler::new(window))
        .with_native_program(redshirt_random_hosted::RandomNativeProgram::new())
        .build();

    let cli_pid = system.execute(&cli_requested_process);

    block_on(event_loop, async move {
        match system.run().await {
            redshirt_core::system::SystemRunOutcome::ProgramFinished { pid, outcome }
                if pid == cli_pid =>
            {
                process::exit(match outcome {
                    Ok(_) => 0,
                    Err(err) => {
                        println!("{:?}", err);
                        1
                    }
                });
            }
            _ => panic!(),
        }
    })
}

fn block_on(
    event_loop: EventLoop<()>,
    future: impl Future<Output = std::convert::Infallible> + 'static,
) -> ! {
    struct Waker {
        proxy: Mutex<winit::event_loop::EventLoopProxy<()>>,
    }

    impl futures::task::ArcWake for Waker {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            let _ = arc_self.proxy.lock().send_event(());
        }
    }

    let waker = futures::task::waker(Arc::new(Waker {
        proxy: Mutex::new(event_loop.create_proxy()),
    }));

    // We're pinning the future here. Ideally we'd pin the future on the stack, but
    // `event_loop::run` requires a `'static` lifetime, and we can't prove to the compiler that
    // the stack content is `'static` without unsafe code.
    let mut future = Box::pin(future);

    event_loop.run(move |event, _, control_flow| {
        println!("test in");
        *control_flow = winit::event_loop::ControlFlow::Wait;

        match event {
            winit::event::Event::WindowEvent {
                event: winit::event::WindowEvent::CloseRequested,
                ..
            } => {
                println!("The close button was pressed; stopping");
                *control_flow = winit::event_loop::ControlFlow::Exit;
            }
            winit::event::Event::MainEventsCleared => {
                match Future::poll(
                    future.as_mut(),
                    &mut futures::task::Context::from_waker(&waker),
                ) {
                    Poll::Ready(v) => match v {}, // unreachable
                    Poll::Pending => {}
                }
            }
            // TODO: handle RedrawRequested as well?
            msg => println!("{:?}", msg), // TODO: remove println
        }

        // FIXME: we get stuck during the polling
        println!("test out");
    })
}
