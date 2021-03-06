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

use p2p_loader::{Network, NetworkConfig};
use std::{env, path::PathBuf};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "passive-node", about = "Redshirt peer-to-peer node.")]
struct CliOptions {
    /// Path to a directory containing Wasm files to automatically push to the DHT.
    #[structopt(long, parse(from_os_str))]
    watch: Vec<PathBuf>,
    /// URL of a git repository whose Wasm files will be automatically pushed to the DHT.
    #[structopt(long)]
    git_watch: Vec<String>,
}

#[cfg(target_arch = "wasm32")]
fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main())
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    env_logger::init();
    futures::executor::block_on(async_main())
}

async fn async_main() {
    let cli_opts = CliOptions::from_args();

    let mut config = NetworkConfig::default();
    config.watched_directories = cli_opts.watch;
    config.watched_git_repositories = cli_opts.git_watch;
    config.private_key = match (env::var("PRIVATE_KEY_BASE"), env::var("NODE_NAME")) {
        (Ok(key_base), Ok(pod_name)) => {
            let mut hasher = blake3::Hasher::new();
            hasher.update(&base64::decode(&key_base).unwrap());
            hasher.update(pod_name.as_bytes());
            Some(hasher.finalize().into())
        }
        _ => None,
    };

    let mut network = Network::<std::convert::Infallible>::start(config).unwrap(); // TODO: use `!`
    loop {
        let _ = network.next_event().await;
    }
}
