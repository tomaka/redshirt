use futures::{
    future::{self, Either, FutureResult},
    prelude::*,
    stream::{self, Chain, IterOk, Once}
};
use futures::compat::Compat;
use get_if_addrs::{IfAddr, get_if_addrs};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
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
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Delay;

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
    type Output = TcpTransStream;
    type Error = io::Error;
    type Listener = TcpListener;
    type ListenerUpgrade = FutureResult<Self::Output, Self::Error>;
    type Dial = TcpDialFut;

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

        let future = TcpDialFut {
            inner: Box::pin(TcpStream::connect(&socket_addr)),
        };

        Ok(future)
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

/// Future that dials a TCP/IP address.
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct TcpDialFut {
    inner: Pin<Box<dyn Future<Output = tcp::TcpStream>>>,
}

impl Future for TcpDialFut {
    type Item = TcpTransStream;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<TcpTransStream, io::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(stream)) => {
                Ok(Async::Ready(TcpTransStream { inner: stream }))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => {
                debug!("Error while dialing => {:?}", err);
                Err(err)
            }
        }
    }
}

/// Wraps around a `TcpStream` and adds logging for important events.
#[derive(Debug)]
pub struct TcpTransStream {
    inner: TcpStream,
}

impl Read for TcpTransStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.inner.read(buf)
    }
}

impl AsyncRead for TcpTransStream {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.inner.prepare_uninitialized_buffer(buf)
    }

    fn read_buf<B: bytes::BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        self.inner.read_buf(buf)
    }
}

impl Write for TcpTransStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.inner.flush()
    }
}

impl AsyncWrite for TcpTransStream {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        AsyncWrite::shutdown(&mut self.inner)
    }
}

impl Drop for TcpTransStream {
    fn drop(&mut self) {
        if let Ok(addr) = self.inner.peer_addr() {
            debug!("Dropped TCP connection to {:?}", addr);
        } else {
            debug!("Dropped TCP connection to undeterminate peer");
        }
    }
}
