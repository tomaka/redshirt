// Copyright(c) 2019 Pierre Krieger

//! TCP/IP.

#![deny(intra_doc_link_resolution_failure)]

// TODO: everything here is a draft

use futures::prelude::*;
use parity_scale_codec::{DecodeAll, Encode as _};
use spin::Mutex;
use std::{io, mem, net::SocketAddr, pin::Pin, sync::Arc, task::Context, task::Poll, task::Waker};

pub mod ffi;

pub struct TcpStream {
    handle: u32,
    /// If Some, we have sent out a "read" message and are waiting for a response.
    has_pending_read: Option<u64>,
    /// If Some, we have sent out a "write" message and are waiting for a response.
    has_pending_write: Option<u64>,
}

impl TcpStream {
    pub fn connect(socket_addr: &SocketAddr) -> impl Future<Output = TcpStream> {
        let tcp_open = ffi::TcpMessage::Open(match socket_addr {
            SocketAddr::V4(addr) => ffi::TcpOpen {
                ip: addr.ip().to_ipv6_mapped().segments(),
                port: addr.port(),
            },
            SocketAddr::V6(addr) => ffi::TcpOpen {
                ip: addr.ip().segments(),
                port: addr.port(),
            },
        });

        async move {
            let message_sink = Arc::new(Mutex::new(Vec::new()));
            let msg_id = syscalls::emit_message(&ffi::INTERFACE, &tcp_open, true)
                .unwrap()
                .unwrap();

            let message = future::poll_fn(move |cx| {
                let mut message_sink_lock = message_sink.lock();
                if message_sink_lock.is_empty() {
                    REACTOR.new_elems.lock().push((msg_id, message_sink.clone(), cx.waker().clone()));
                    let futex_wake = threads::ffi::ThreadsMessage::FutexWake(threads::ffi::FutexWake {
                        addr: &REACTOR.notify_futex as *const u32 as usize as u32,
                        nwake: 1,
                    });
                    syscalls::emit_message(&threads::ffi::INTERFACE, &futex_wake, false).unwrap();
                    return Poll::Pending;
                }
                Poll::Ready(mem::replace(&mut *message_sink_lock, Vec::new()))
            }).await;

            let message: ffi::TcpOpenResponse = DecodeAll::decode_all(&message).unwrap();
            let handle = message.result.unwrap();

            TcpStream {
                handle,
                has_pending_read: None,
                has_pending_write: None,
            }
        }
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        // TODO: for now we're always blocking because we don't have threads
        let tcp_read = ffi::TcpMessage::Read(ffi::TcpRead {
            socket_id: self.handle,
        });
        let msg_id = syscalls::emit_message(&ffi::INTERFACE, &tcp_read, true)
            .unwrap()
            .unwrap();
        let msg = syscalls::next_message(&mut [msg_id], true).unwrap();
        let result = match msg {
            // TODO: code style: improve syscall's API
            syscalls::Message::Response(syscalls::ffi::ResponseMessage {
                message_id,
                actual_data,
                ..
            }) => {
                assert_eq!(message_id, msg_id);
                let msg: ffi::TcpReadResponse = DecodeAll::decode_all(&actual_data).unwrap();
                msg.result
            }
            _ => unreachable!(),
        };

        let data = match result {
            Ok(d) => d,
            Err(_) => return Poll::Ready(Err(io::ErrorKind::Other.into())), // TODO:
        };

        buf[..data.len()].copy_from_slice(&data); // TODO: this just assumes that buf is large enough
        Poll::Ready(Ok(data.len()))
    }

    // TODO: unsafe fn initializer(&self) -> Initializer { ... }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        // TODO: for now we're always blocking because we don't have threads
        let tcp_write = ffi::TcpMessage::Write(ffi::TcpWrite {
            socket_id: self.handle,
            data: buf.to_vec(),
        });
        let msg_id = syscalls::emit_message(&ffi::INTERFACE, &tcp_write, true)
            .unwrap()
            .unwrap();
        let msg = syscalls::next_message(&mut [msg_id], true).unwrap();
        let result = match msg {
            // TODO: code style: improve syscall's API
            syscalls::Message::Response(syscalls::ffi::ResponseMessage {
                message_id,
                actual_data,
                ..
            }) => {
                assert_eq!(message_id, msg_id);
                let msg: ffi::TcpWriteResponse = DecodeAll::decode_all(&actual_data).unwrap();
                msg.result
            }
            _ => unreachable!(),
        };

        match result {
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(io::ErrorKind::Other.into())), // TODO:
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        let tcp_close = ffi::TcpMessage::Close(ffi::TcpClose {
            socket_id: self.handle,
        });

        syscalls::emit_message(&ffi::INTERFACE, &tcp_close, false);
    }
}

lazy_static::lazy_static! {
    static ref REACTOR: Reactor = {
        threads::spawn_thread(|| background_thread());

        Reactor {
            notify_futex: 0,
            new_elems: Mutex::new(Vec::with_capacity(16))
        }
    };
}

struct Reactor {
    notify_futex: u32,
    new_elems: Mutex<Vec<(u64, Arc<Mutex<Vec<u8>>>, Waker)>>,
}

fn background_thread() {
    let mut message_ids = vec![0];
    let mut wakers = Vec::with_capacity(16);

    loop {
        let mut new_elems = REACTOR.new_elems.lock();

        // Basic cleanup in order to release memory acquired during peaks.
        if message_ids.capacity() - message_ids.len() >= 32 {
            message_ids.shrink_to_fit();
        }

        // We want to be notified whenever the non-background thread adds elements to the
        // `Reactor`.
        let wait_notify = {
            let msg = threads::ffi::ThreadsMessage::FutexWait(threads::ffi::FutexWait {
                addr: &REACTOR.notify_futex as *const u32 as usize as u32,
                val_cmp: 0,
            });
            syscalls::emit_message(&threads::ffi::INTERFACE, &msg, true).unwrap().unwrap()
        };

        message_ids[0] = wait_notify;

        for (msg_id, sink, waker) in new_elems.drain(..) {
            // TODO: is it possible that we get a message id for a message that's already been responsed? figure this out
            if let Some(existing_pos) = message_ids.iter().position(|m| *m == msg_id) {
                wakers[existing_pos] = (sink, waker);
            } else {
                message_ids.push(msg_id);
                wakers.push((sink, waker));
            }
        }

        debug_assert!(new_elems.is_empty());
        // TODO: new_elems.shrink_to(16);

        loop {
            let msg = match syscalls::next_message(&mut message_ids, true) {
                Some(syscalls::Message::Response(msg)) => msg,
                Some(syscalls::Message::Interface(_)) => unreachable!(),
                None => unreachable!(),
            };

            if msg.message_id == wait_notify {
                debug_assert_eq!(msg.index_in_list, 0);
                break;
            }

            debug_assert_ne!(msg.index_in_list, 0);
            message_ids.remove(msg.index_in_list as usize);
            
            let (sink, waker) = wakers.remove(msg.index_in_list as usize - 1);
            *sink.lock() = msg.actual_data;
            waker.wake();
        }
    }
}
