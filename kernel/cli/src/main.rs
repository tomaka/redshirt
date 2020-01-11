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

use futures::prelude::*;
use std::{fs, path::PathBuf, process};
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
    let event_loop = EventLoop::new();

    let cli_requested_process = {
        let cli_opts = CliOptions::from_args();
        let wasm_file_content = fs::read(cli_opts.wasm_file).expect("failed to read input file");
        redshirt_core::module::Module::from_bytes(&wasm_file_content)
            .expect("failed to parse input file")
    };

    let window = winit::window::Window::new(&event_loop).unwrap();

    let mut system = redshirt_core::system::SystemBuilder::new()
        .with_native_program(redshirt_time_hosted::TimerHandler::new())
        .with_native_program(redshirt_stdout_hosted::StdoutHandler::new())
        .with_native_program(redshirt_webgpu_hosted::WebGPUHandler::new(window))
        .build();

    let cli_pid = system.execute(&cli_requested_process);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = winit::event_loop::ControlFlow::Poll;

        match event {
            winit::event::Event::WindowEvent {
                event: winit::event::WindowEvent::CloseRequested,
                ..
            } => {
                println!("The close button was pressed; stopping");
                *control_flow = winit::event_loop::ControlFlow::Exit;
            },
            winit::event::Event::MainEventsCleared => {
                // TODO: put application code here only
            },
            _ => {}     // TODO: RedrawRequested as well
        }

        let outcome = system.run().now_or_never();
        match outcome {
            None => {},
            Some(redshirt_core::system::SystemRunOutcome::ProgramFinished { pid, outcome })
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
    });
}
