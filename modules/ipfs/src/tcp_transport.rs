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

use futures::{prelude::*, compat::Compat};
use futures01::Future;
use libp2p_core::{
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{ListenerEvent, TransportError}
};
use log::{debug, trace};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    iter::{self, FromIterator},
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
    vec::IntoIter
};

/// Represents the configuration for a TCP/IP transport capability for libp2p.
#[derive(Debug, Clone, Default)]
pub struct TcpConfig {
}

impl TcpConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> TcpConfig {
        TcpConfig {
        }
    }
}

impl Transport for TcpConfig {
    type Output = Compat<nametbd_tcp_interface::TcpStream>;
    type Error = io::Error;
    type Listener = Box<dyn futures01::Stream<Item = ListenerEvent<Self::ListenerUpgrade>, Error = Self::Error> + Send>;
    type ListenerUpgrade = futures01::future::FutureResult<Self::Output, Self::Error>;
    type Dial = Box<dyn futures01::Future<Item = Self::Output, Error = io::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        unimplemented!()
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

        debug!("Dialing {}", addr);
        Ok(Box::new(Future::map(Compat::new(Box::pin(nametbd_tcp_interface::TcpStream::connect(&socket_addr).map(Ok))), |f| Compat::new(f))))
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
