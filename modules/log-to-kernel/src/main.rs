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
            let level = match message.level() {
                ffi::Level::Error => "ERR ",
                ffi::Level::Warn => "WARN",
                ffi::Level::Info => "INFO",
                ffi::Level::Debug => "DEBG",
                ffi::Level::Trace => "TRCE",
            };

            let kernel_message =
                format!("[{:?}] [{}] {}", msg.emitter_pid, level, message.message());
            redshirt_kernel_log_interface::log(kernel_message.as_bytes());
        } else {
            let kernel_message = format!("[{:?}] Bad log message", msg.emitter_pid);
            redshirt_kernel_log_interface::log(kernel_message.as_bytes());
        }
    }
}
