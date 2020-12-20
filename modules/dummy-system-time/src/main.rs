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

use redshirt_interface_interface::DecodedInterfaceOrDestroyed;
use redshirt_syscalls::Decode as _;
use redshirt_system_time_interface::ffi as sys_time_ffi;

fn main() {
    redshirt_syscalls::block_on(async_main())
}

async fn async_main() {
    let mut registration = redshirt_interface_interface::register_interface(sys_time_ffi::INTERFACE)
        .await
        .unwrap();

    loop {
        let interface_event = registration.next_message_raw().await;
        let msg = match interface_event {
            DecodedInterfaceOrDestroyed::Interface(msg) => msg,
            DecodedInterfaceOrDestroyed::ProcessDestroyed(_) => continue,
        };

        let msg_data = sys_time_ffi::TimeMessage::decode(msg.actual_data).unwrap();
        let sys_time_ffi::TimeMessage::GetSystem = msg_data;

        if let Some(id) = msg.message_id {
            redshirt_syscalls::emit_answer(id, &0u128);
        }
    }
}
