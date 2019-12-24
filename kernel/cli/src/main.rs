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

use futures::{channel::mpsc, pin_mut, prelude::*};
use parity_scale_codec::DecodeAll;
use std::{fs, path::PathBuf, process, sync::Arc};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "redshirt", about = "Redshirt modules executor.")]
struct CliOptions {
    /// Input file.
    #[structopt(parse(from_os_str))]
    input: Option<PathBuf>,
}

fn main() {
    futures::executor::block_on(async_main());
}

async fn async_main() {
    let cli_requested_process = {
        let cli_opts = CliOptions::from_args();
        if let Some(input) = cli_opts.input {
            let file_content = fs::read(input).expect("failed to read input file");
            Some(
                redshirt_core::module::Module::from_bytes(&file_content)
                    .expect("failed to parse input file"),
            )
        } else {
            None
        }
    };

    let mut system =
        redshirt_wasi_hosted::register_extrinsics(redshirt_core::system::SystemBuilder::<
            redshirt_wasi_hosted::WasiExtrinsic,
        >::new())
        /*.with_native_program(redshirt_time_hosted::TimerHandler::new())
        .with_native_program(redshirt_tcp_hosted::TcpState::new())*/
        .with_native_program(redshirt_stdout_hosted::StdoutHandler::new())
        .build();

    let cli_pid = if let Some(cli_requested_process) = cli_requested_process {
        Some(system.execute(&cli_requested_process))
    } else {
        None
    };

    //let mut wasi = redshirt_wasi_hosted::WasiStateMachine::new();

    loop {
        let outcome = system.run().await;
        match outcome {
            redshirt_core::system::SystemRunOutcome::ThreadWaitExtrinsic {
                pid,
                thread_id,
                extrinsic,
                params,
            } => {
                panic!()
                /*let out =
                    wasi.handle_extrinsic_call(&mut system, extrinsic, pid, thread_id, params);
                if let redshirt_wasi_hosted::HandleOut::EmitMessage {
                    id,
                    interface,
                    message,
                } = out
                {
                    if interface == redshirt_stdout_interface::ffi::INTERFACE {
                        let msg =
                            redshirt_stdout_interface::ffi::StdoutMessage::decode_all(&message);
                        let redshirt_stdout_interface::ffi::StdoutMessage::Message(msg) =
                            msg.unwrap();
                        print!("{}", msg);
                    /*} else if interface == redshirt_time_interface::ffi::INTERFACE {
                        if let Some(answer) =
                            time.time_message(id.map(MessageId::Wasi), &message)
                        {
                            unimplemented!()
                        }
                    } else if interface == redshirt_tcp_interface::ffi::INTERFACE {
                        let message: redshirt_tcp_interface::ffi::TcpMessage =
                            DecodeAll::decode_all(&message).unwrap();
                        tcp.handle_message(id.map(MessageId::Wasi), message).await;*/
                    } else {
                        panic!()
                    }
                }*/
            }
            redshirt_core::system::SystemRunOutcome::ProgramFinished { pid, outcome } => {
                if cli_pid == Some(pid) {
                    process::exit(match outcome {
                        Ok(_) => 0,
                        Err(err) => {
                            println!("{:?}", err);
                            1
                        }
                    });
                }
            }
            _ => panic!(),
        }
    }
}

enum MessageId {
    Core(redshirt_syscalls_interface::MessageId),
    Wasi(redshirt_wasi_hosted::WasiMessageId),
}
