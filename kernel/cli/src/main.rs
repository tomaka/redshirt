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

use std::{fs, path::PathBuf, process};
use structopt::StructOpt;

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
    let cli_requested_process = {
        let cli_opts = CliOptions::from_args();
        let wasm_file_content = fs::read(cli_opts.wasm_file).expect("failed to read input file");
        redshirt_core::module::Module::from_bytes(&wasm_file_content)
            .expect("failed to parse input file")
    };

    let system = redshirt_core::system::SystemBuilder::new()
        .with_native_program(redshirt_time_hosted::TimerHandler::new())
        .with_native_program(redshirt_tcp_hosted::TcpHandler::new())
        .with_native_program(redshirt_log_hosted::LogHandler::new())
        .build();

    let cli_pid = system.execute(&cli_requested_process);

    loop {
        let outcome = system.run().await;
        match outcome {
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
    }
}
