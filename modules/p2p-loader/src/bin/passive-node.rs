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

use p2p_loader::{Network, NetworkConfig};
use std::env;

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
    let config = NetworkConfig {
        private_key: if let Ok(key) = env::var("PRIVATE_KEY") {
            let bytes = base64::decode(&key).unwrap();
            assert_eq!(bytes.len(), 32);
            let mut out = [0; 32];
            out.copy_from_slice(&bytes);
            Some(out)
        } else {
            None
        },
    };

    let mut network = Network::<std::convert::Infallible>::start(config); // TODO: use `!`

    loop {
        let _ = network.next_event().await;
    }
}
