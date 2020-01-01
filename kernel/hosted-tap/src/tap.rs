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

//! Convenient API around the TAP interface.

use futures::{channel::mpsc, lock::Mutex, prelude::*};
use mio::{unix::EventedFd, Evented, Poll as MPoll, PollOpt, Ready, Token};
use std::{io, os::unix::io::AsRawFd as _, thread, time::Duration};
use tokio::{io::PollEvented, prelude::*, runtime::Runtime, time};

pub struct TapInterface {
    /// Sender for messages to output on the TAP interface.
    ///
    /// Uses a `Buffer` in order to be able to make sure that sends are going to succeed.
    // TODO: if `mpsc::Sender` gets a `is_ready()` function or something, we can get rid of
    // the `Buffer`
    to_send: mpsc::Sender<Vec<u8>>,
    /// Receiver for messages coming from the TAP interface.
    recv: Mutex<mpsc::Receiver<Vec<u8>>>,
}

struct MioWrapper {
    inner: tun_tap::Iface,
}

impl TapInterface {
    /// Initializes a new TAP interface.
    ///
    /// > **Note**: It is extremely common for this method to fail because of lack of
    /// >           privilege. It might be a good idea to **not** unwrap this `Result`.
    pub fn new() -> Result<Self, io::Error> {
        let (to_send, mut to_send_rx) = mpsc::channel::<Vec<u8>>(4);
        let (mut recv_tx, recv) = mpsc::channel(4);

        let interface = tun_tap::Iface::without_packet_info("redshirt-0", tun_tap::Mode::Tap)?;
        // TODO: yeah, no
        let mut nonblock: libc::c_int = 1;
        let result = unsafe { libc::ioctl(interface.as_raw_fd(), libc::FIONBIO, &mut nonblock) };
        assert_eq!(result, 0);

        let mut tokio_runtime = Runtime::new()?;

        // We don't want users to be forced to use a tokio runtime, so we spawn a background
        // thread.
        thread::Builder::new()
            .name("tap-interface".to_string())
            .spawn(move || {
                let result: Result<(), io::Error> = tokio_runtime.block_on(async move {
                    let mut interface = PollEvented::new(MioWrapper { inner: interface })?;
                    let mut read_buf = [0; 1542];

                    loop {
                        match future::select(interface.read(&mut read_buf), to_send_rx.next()).await
                        {
                            future::Either::Left((Ok(n), _)) => {
                                let buffer = read_buf[..n].to_owned();
                                println!(
                                    "rx: {:?}",
                                    buffer
                                        .iter()
                                        .map(|b| format!("{:x}", *b))
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                );
                                if recv_tx.send(buffer).await.is_err() {
                                    break Ok(());
                                }
                            }
                            future::Either::Right((Some(to_send), _)) => {
                                println!(
                                    "tx: {:?}",
                                    to_send
                                        .iter()
                                        .map(|b| format!("{:x}", *b))
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                );
                                match time::timeout(
                                    Duration::from_secs(5),
                                    interface.write_all(&to_send),
                                )
                                .await
                                {
                                    Ok(Ok(())) => {}
                                    Err(err) => break Err(io::Error::from(err)),
                                    Ok(Err(err)) => break Err(err),
                                }
                            }
                            future::Either::Left((Err(err), _)) => break Err(err),
                            future::Either::Right((None, _)) => break Ok(()),
                        }
                    }
                });

                if let Err(err) = result {
                    panic!("Error in TAP interface: {}", err);
                }
            })?;

        Ok(TapInterface {
            to_send,
            recv: Mutex::new(recv),
        })
    }

    /// Try to send an Ethernet packet on the TAP interface.
    ///
    /// > **Important**: Packets are discarded if they come too quickly.
    // TODO: take this into account in the network manager ^
    pub fn send(&self, buffer: Vec<u8>) {
        match self.to_send.clone().try_send(buffer) {
            Ok(()) => return,
            Err(err) => assert!(err.is_full()),
        }
    }

    /// If true, then calling `send` will not discard the packet we give to it.
    ///
    /// > **Note**: This API is obviously racy, as a separate thread could call `send` right after
    /// >           `is_ready_to_send` returned.
    pub fn is_ready_to_send(&self) -> bool {
        let waker = futures::task::noop_waker();
        let mut ctxt = futures::task::Context::from_waker(&waker);
        self.to_send.clone().poll_ready(&mut ctxt).is_ready()
    }

    /// Receives the next packet coming from the TAP interface.
    pub async fn recv(&self) -> Vec<u8> {
        let mut recv = self.recv.lock().await;
        recv.next().await.unwrap()
    }
}

impl Evented for MioWrapper {
    fn register(
        &self,
        poll: &MPoll,
        token: Token,
        events: Ready,
        opts: PollOpt,
    ) -> Result<(), io::Error> {
        EventedFd(&self.inner.as_raw_fd()).register(poll, token, events, opts)
    }

    fn reregister(
        &self,
        poll: &MPoll,
        token: Token,
        events: Ready,
        opts: PollOpt,
    ) -> Result<(), io::Error> {
        EventedFd(&self.inner.as_raw_fd()).reregister(poll, token, events, opts)
    }

    fn deregister(&self, poll: &MPoll) -> Result<(), io::Error> {
        EventedFd(&self.inner.as_raw_fd()).deregister(poll)
    }
}

impl io::Read for MioWrapper {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.inner.recv(buf)
    }
}

impl io::Write for MioWrapper {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.inner.send(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        Ok(())
    }
}
