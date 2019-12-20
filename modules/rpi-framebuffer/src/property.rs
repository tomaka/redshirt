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

use crate::mailbox;
use core::convert::TryFrom as _;

#[repr(align(16))]
struct Packet1 {
    data: [u32; 20],
}

#[repr(align(16))]
struct Packet2 {
    data: [u32; 8],
}

pub async fn init() {
    let buffer1 = redshirt_hardware_interface::malloc::PhysicalBuffer::new(Packet1 {
        data: [
            80,                             // The whole buffer is 80 bytes
            0,                              // This is a request, so the request/response code is 0
            0x00048003, 8, 0, 640, 480,     // This tag sets the screen size to 640x480
            0x00048004, 8, 0, 640, 480,     // This tag sets the virtual screen size to 640x480
            0x00048005, 4, 0, 24,           // This tag sets the depth to 24 bits
            0,                              // This is the end tag
            0, 0, 0                         // This pads the message to by 16 byte aligned
        ]            
    }).await;
    panic!();

    assert_eq!(buffer1.pointer() % 16, 0);
    mailbox::write_mailbox(mailbox::Message {
        channel: 8,
        data: u32::try_from(buffer1.pointer() >> 4).unwrap(),
    });

    mailbox::read_mailbox().await;

    let data1 = buffer1.take().await;
    assert!(data1.data[1] == 0x80000000);
    panic!();

    let buffer2 = redshirt_hardware_interface::malloc::PhysicalBuffer::new(Packet2 {
        data: [
            32,                         // The whole buffer is 32 bytes
            0,                          // This is a request, so the request/response code is 0
            0x00040001, 8, 0, 16, 0,    // This tag requests a 16 byte aligned framebuffer
            0                           // This is the end tag
        ]            
    }).await;

    assert_eq!(buffer2.pointer() % 16, 0);
    mailbox::write_mailbox(mailbox::Message {
        channel: 8,
        data: u32::try_from(buffer2.pointer() >> 4).unwrap(),
    });

    mailbox::read_mailbox().await;
}
