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

use futures::prelude::*;
use p2p_loader::{Network, NetworkEvent};
use parity_scale_codec::DecodeAll;

fn main() {
    // TODO: too verbose
    //redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main())
}

async fn async_main() {
    // `Some` if the current process has already registered itself as the "loader" interface
    // handler.
    let mut registration = None::<redshirt_interface_interface::Registration>;

    let mut network = Network::start(Default::default()).unwrap();

    loop {
        let event = {
            let next_interface = async {
                if let Some(registration) = registration.as_mut() {
                    registration.next_message_raw().await
                } else {
                    loop {
                        futures::pending!()
                    }
                }
            };
            let next_net_event = network.next_event();
            futures::pin_mut!(next_interface, next_net_event);
            match future::select(next_interface, next_net_event).await {
                future::Either::Left((v, _)) => future::Either::Left(v),
                future::Either::Right((v, _)) => future::Either::Right(v),
            }
        };

        let msg = match event {
            future::Either::Left(
                redshirt_interface_interface::DecodedInterfaceOrDestroyed::Interface(m),
            ) => m,
            future::Either::Left(
                redshirt_interface_interface::DecodedInterfaceOrDestroyed::ProcessDestroyed(_),
            ) => continue,
            future::Either::Right(NetworkEvent::Readiness(true)) => {
                if registration.is_none() {
                    registration = redshirt_interface_interface::register_interface(
                        redshirt_loader_interface::ffi::INTERFACE,
                    )
                    .await
                    .ok();
                }
                continue;
            }
            future::Either::Right(NetworkEvent::Readiness(false)) => {
                continue;
            }
            future::Either::Right(NetworkEvent::FetchSuccess { data, user_data }) => {
                assert!(registration.is_some());
                let rp = redshirt_loader_interface::ffi::LoadResponse { result: Ok(data) };
                redshirt_interface_interface::emit_answer(user_data, &rp);
                continue;
            }
            future::Either::Right(NetworkEvent::FetchFail { user_data }) => {
                assert!(registration.is_some());
                let rp = redshirt_loader_interface::ffi::LoadResponse { result: Err(()) };
                redshirt_interface_interface::emit_answer(user_data, &rp);
                continue;
            }
        };

        assert!(registration.is_some());
        assert_eq!(msg.interface, redshirt_loader_interface::ffi::INTERFACE);
        let msg_data =
            redshirt_loader_interface::ffi::LoaderMessage::decode_all(&msg.actual_data.0).unwrap();
        let redshirt_loader_interface::ffi::LoaderMessage::Load(hash_to_load) = msg_data;
        log::info!("loading {}", bs58::encode(hash_to_load).into_string());
        network.start_fetch(&hash_to_load, msg.message_id.unwrap());
    }
}
