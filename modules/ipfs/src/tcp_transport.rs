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

use futures::prelude::*;
use libp2p_core::{
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{ListenerEvent, TransportError}
};
use log::debug;
use std::{io, iter, net::IpAddr, net::SocketAddr, pin::Pin};

/// Represents the configuration for a TCP/IP transport capability for libp2p.
#[derive(Debug, Clone)]
pub struct TcpConfig {
}

impl TcpConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> TcpConfig {
        TcpConfig {
        }
    }
}

impl Default for TcpConfig {
    fn default() -> Self {
        TcpConfig::new()
    }
}

impl Transport for TcpConfig {
    type Output = nametbd_tcp_interface::TcpStream;
    type Error = io::Error;
    type Listener = Pin<Box<dyn Stream<Item = Result<ListenerEvent<Self::ListenerUpgrade>, Self::Error>> + Send>>;
    type ListenerUpgrade = future::Ready<Result<Self::Output, Self::Error>>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, io::Error>> + Send>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let socket_addr =
            if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
                socket_addr
            } else {
                return Err(TransportError::MultiaddrNotSupported(addr))
            };

        Ok(Box::pin(async move {
            let listener = nametbd_tcp_interface::TcpListener::bind(&socket_addr).await
                .map_err(|()| io::Error::from(io::ErrorKind::Other))?;
            let local_addr = ip_to_multiaddr(listener.local_addr().ip(), listener.local_addr().port());
            println!("Listening on {}", local_addr);

            let first = stream::once({
                let local_addr = local_addr.clone();
                async move {
                    Ok(ListenerEvent::NewAddress(local_addr))
                }
            });

            let then = stream::unfold(listener, move |mut s| {
                let local_addr = local_addr.clone();
                async move {
                    let (socket, remote_addr) = s.accept().await;
                    let ev = ListenerEvent::Upgrade {
                        upgrade: future::ready(Ok(socket)),
                        local_addr: local_addr.clone(),
                        remote_addr: ip_to_multiaddr(remote_addr.ip(), remote_addr.port()),
                    };
                    Some((Ok(ev), s))
                }
            });

            Ok(first.chain(then))
        }.try_flatten_stream()))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr =
            if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
                if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                    debug!("Instantly refusing dialing {}, as it is invalid", addr);
                    return Err(TransportError::Other(io::ErrorKind::ConnectionRefused.into()))
                }
                socket_addr
            } else {
                return Err(TransportError::MultiaddrNotSupported(addr))
            };

        println!("Dialing {}", addr);
        Ok(Box::pin(async move {
            nametbd_tcp_interface::TcpStream::connect(&socket_addr).await
                .map_err(|()| io::Error::from(io::ErrorKind::Other))
        }))
    }
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Result<SocketAddr, ()> {
    let mut iter = addr.iter();
    let proto1 = iter.next().ok_or(())?;
    let proto2 = iter.next().ok_or(())?;

    if iter.next().is_some() {
        return Err(());
    }

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        _ => Err(()),
    }
}

/// Create a [`Multiaddr`] from the given IP address and port number.
fn ip_to_multiaddr(ip: IpAddr, port: u16) -> Multiaddr {
    let proto = match ip {
        IpAddr::V4(ip) => Protocol::Ip4(ip),
        IpAddr::V6(ip) => Protocol::Ip6(ip)
    };

    iter::once(proto).chain(iter::once(Protocol::Tcp(port))).collect()
}
