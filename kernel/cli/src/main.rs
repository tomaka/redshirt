// Copyright (C) 2019-2021  Pierre Krieger
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

use futures::{channel::mpsc, prelude::*};
use redshirt_core::{build_wasm_module, extrinsics::wasi::WasiExtrinsics, module::ModuleHash};
use std::{fs, path::PathBuf, process, sync::Arc};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "redshirt-cli", about = "Redshirt modules executor.")]
struct CliOptions {
    /// WASM file to run.
    #[structopt(long, parse(from_os_str))]
    module_path: Vec<PathBuf>,

    /// WASM file to run in the background.
    ///
    /// Contrary to `module_path`, the kernel will not stop if this module stops.
    #[structopt(long, parse(from_os_str))]
    background_module_path: Vec<PathBuf>,

    /// Base58 encoding of the blake3 hash of a module to run.
    ///
    /// The module will be fetched from the public network.
    #[structopt(long, parse(try_from_str = ModuleHash::from_base58))]
    module_hash: Vec<ModuleHash>,

    /// Base58 encoding of the blake3 hash of a module to run in the background.
    ///
    /// The module will be fetched from the public network.
    ///
    /// Contrary to `module_hash`, the kernel will not stop if this module stops.
    #[structopt(long, parse(try_from_str = ModuleHash::from_base58))]
    background_module_hash: Vec<ModuleHash>,
}

fn main() {
    let cli_opts = CliOptions::from_args();

    let mut cli_requested_processes = Vec::new();

    for module_path in cli_opts.module_path {
        let wasm_file_content = fs::read(&module_path).expect("failed to read input file");
        let module = redshirt_core::module::Module::from_bytes(&wasm_file_content)
            .expect("failed to parse input file");
        cli_requested_processes.push((module_path, module, true));
    }

    for module_path in cli_opts.background_module_path {
        let wasm_file_content = fs::read(&module_path).expect("failed to read input file");
        let module = redshirt_core::module::Module::from_bytes(&wasm_file_content)
            .expect("failed to parse input file");
        cli_requested_processes.push((module_path, module, false));
    }

    let framebuffer_context = redshirt_framebuffer_hosted::FramebufferContext::new();

    let system = redshirt_core::system::SystemBuilder::new(WasiExtrinsics::default(), rand::random())
        .with_native_program(redshirt_tcp_hosted::TcpHandler::new())
        .with_native_program(redshirt_log_hosted::LogHandler::new())
        .with_native_program(redshirt_framebuffer_hosted::FramebufferHandler::new(
            &framebuffer_context,
        ))
        .with_native_program(redshirt_random_hosted::RandomNativeProgram::new())
        .with_startup_process(build_wasm_module!(
            "../../../modules/p2p-loader",
            "modules-loader"
        ))
        .with_main_programs(cli_opts.module_hash)
        .with_main_programs(cli_opts.background_module_hash)
        .build()
        .expect("Failed to start system");

    let mut cli_pids = Vec::with_capacity(cli_requested_processes.len());
    // TODO: should also contain the `module_hash`es
    for (module_path, module, foreground) in cli_requested_processes {
        match system.execute(&module) {
            Ok(pid) if foreground => cli_pids.push(pid),
            Ok(_) => {}
            Err(err) => panic!("Failed to load {}: {}", module_path.display(), err),
        }
    }

    // TODO: uncomment after cli_pids contains the `module_hash`es
    /*if cli_pids.is_empty() {
        return;
    }*/

    // We now spawn background tasks that run the scheduler.
    // Background tasks report all events to the main thread, which can then decide to stop
    // everything.
    let (tx, mut rx) = mpsc::channel(16);
    let system = Arc::new(system);

    for _ in 0..num_cpus::get() {
        let mut tx = tx.clone();
        let system = system.clone();
        async_std::task::spawn(async move {
            loop {
                match system.run().await {
                    redshirt_core::system::SystemRunOutcome::ProgramFinished { pid, outcome } => {
                        if tx.send((pid, outcome)).await.is_err() {
                            break;
                        }
                    }
                    redshirt_core::system::SystemRunOutcome::KernelDebugMetricsRequest(report) => {
                        report.respond("");
                    }
                }
            }
        });
    }

    // All the background tasks events are grouped together and sent here.
    framebuffer_context.run(async move {
        while let Some((pid, outcome)) = rx.next().await {
            match outcome {
                Err(err) if cli_pids.iter().any(|p| *p == pid) => {
                    eprintln!("{:?}", err);
                    process::exit(1);
                }
                Ok(()) => {
                    cli_pids.retain(|p| *p != pid);
                    if cli_pids.is_empty() {
                        process::exit(0);
                    }
                }
                Err(_) => {}
            }
        }
    });
}
