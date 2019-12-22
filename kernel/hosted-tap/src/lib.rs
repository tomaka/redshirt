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

//! Registers a network interface that uses
//! [TAP](https://en.wikipedia.org/wiki/TAP_(network_driver)).

// Implementation notes:
//
// Since reading and writing from/to the TAP interface is blocking, we spawn two background threads
// dedicated to these operations.

use futures::{channel::mpsc, executor::block_on, prelude::*};
use redshirt_syscalls_interface::MessageId;
use std::{io, sync::Arc, thread};

pub struct TapNetworkInterface {
    to_send: mpsc::Sender<Vec<u8>>,
    recv: mpsc::Receiver<Vec<u8>>,
    /// If true, we have already sent the registration message to the network interface.
    registered: bool,
}

impl TapNetworkInterface {
    /// Initializes a new TAP interface.
    ///
    /// > **Note**: It is extremely common for this method to fail because of lack of
    /// >           priviledges. It might be a good idea to **not** unwrap this `Result`.
    ///
    pub fn new() -> Result<TapNetworkInterface, io::Error> {
        let (to_send, mut to_send_rx) = mpsc::channel(4);
        let (mut recv_tx, recv) = mpsc::channel(4);

        let interface = Arc::new({ tun_tap::Iface::new("redshirt-%d", tun_tap::Mode::Tap)? });

        thread::Builder::new()
            .name("tap-sender".to_string())
            .spawn({
                let interface = interface.clone();
                move || {
                    loop {
                        let packet: Vec<u8> = match block_on(to_send_rx.next()) {
                            None => break, // The `TapNetworkInterface` has been dropped.
                            Some(p) => p,
                        };

                        if interface.send(&packet).is_err() {
                            // Error on the tap interface. Killing this thread will close the
                            // channel and thus inform the `TapNetworkInterface` that something
                            // bad happened.
                            break;
                        }
                    }
                }
            })?;

        thread::Builder::new()
            .name("tap-receiver".to_string())
            .spawn(move || {
                let mut read_buffer = vec![0; 1542];

                loop {
                    let buffer = match interface.recv(&mut read_buffer) {
                        Ok(n) => read_buffer[..n].to_owned(),
                        Err(_) => {
                            // Error on the tap interface. Killing this thread will close the
                            // channel and thus inform the `TapNetworkInterface` that something
                            // bad happened.
                            break;
                        }
                    };

                    if block_on(recv_tx.send(buffer)).is_err() {
                        // The `TapNetworkInterface` has been dropped.
                        break;
                    }
                }
            })?;

        Ok(TapNetworkInterface {
            to_send,
            recv,
            registered: false,
        })
    }

    pub async fn next_event(&mut self) -> (MessageId, Vec<u8>) {
        unimplemented!()
    }
}
