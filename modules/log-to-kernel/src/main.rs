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

//! Implements the log interface by redirecting the logs as kernel logs.

use redshirt_log_interface::ffi;
use redshirt_syscalls::{Decode, EncodedMessage};
use std::{convert::TryFrom as _, fmt, sync::atomic};

fn main() {
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() -> ! {
    redshirt_interface_interface::register_interface(ffi::INTERFACE)
        .await
        .unwrap();

    loop {
        let msg = match redshirt_syscalls::next_interface_message().await {
            redshirt_syscalls::DecodedInterfaceOrDestroyed::Interface(m) => m,
            redshirt_syscalls::DecodedInterfaceOrDestroyed::ProcessDestroyed(_) => continue,
        };

        assert_eq!(msg.interface, ffi::INTERFACE);

        if let Ok(message) = ffi::DecodedLogMessage::decode(msg.actual_data) {
            redshirt_kernel_log_interface::log(message.message().as_bytes());
        }
        // TODO: show the PID and log level, as commented out below
        /*if let Ok(message) = ffi::DecodedLogMessage::decode(msg.actual_data) {
            let level = match message.level() {
                ffi::Level::Error => b"ERR ",
                ffi::Level::Warn => b"WARN",
                ffi::Level::Info => b"INFO",
                ffi::Level::Debug => b"DEBG",
                ffi::Level::Trace => b"TRCE",
            };

            write_utf8_bytes(b"[").await;
            write_utf8_bytes(format!("{:?}", msg.emitter_pid).as_bytes()).await;
            write_utf8_bytes(b"] [").await;
            write_utf8_bytes(level).await;
            write_utf8_bytes(b"] ").await;
            write_untrusted_str(message.message()).await;
            write_utf8_bytes(b"\n").await;
        } else {
            write_utf8_bytes(b"[").await;
            write_untrusted_str(&format!("{:?}", msg.emitter_pid)).await;
            write_utf8_bytes(b"] Bad log message\n").await;
        }*/
    }
}
