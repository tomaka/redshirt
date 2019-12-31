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
use std::{borrow::Cow, convert::TryFrom as _, iter};

/// Builder for a request on the property interface.
pub struct PropertyMessageBuilder {
    /// Buffer containing the request.
    ///
    /// The first element (the buffer length) is only filled right before we submit the message.
    buffer: Vec<u32>,
}

impl PropertyMessageBuilder {
    pub fn new() -> PropertyMessageBuilder {
        PropertyMessageBuilder {
            buffer: vec![0; 2],
        }
    }

    pub fn add_tag(&mut self, tag: u32, value_buffer: &[u8]) {
        let value_buffer_size_bytes = u32::try_from(value_buffer.len()).unwrap();
        let value_buffer_num_u32s = value_buffer_size_bytes
            .checked_sub(1)
            .map(|v| 4 * (1 + (v / 4)))
            .unwrap_or(0);

        let buffer_tag_start = self.buffer.len();
        let new_buffer_len = self.buffer.len() + 3 + usize::try_from(value_buffer_num_u32s).unwrap();
        self.buffer.resize(new_buffer_len, 0);

        self.buffer[buffer_tag_start] = tag;
        self.buffer[buffer_tag_start + 1] = value_buffer_size_bytes;

        // Copy `value_buffer` into `self.buffer`.
        for (byte_index, byte) in value_buffer.iter().enumerate() {
            self.buffer[buffer_tag_start + 3 + (byte_index / 4)] |= u32::from(*byte) << (8 * (byte_index % 4));
        }
    }

    pub fn with_tag(mut self, tag: u32, value_buffer: &[u8]) -> Self {
        self.add_tag(tag, value_buffer);
        self
    }

    pub async fn send(self) {
        unimplemented!()
        /*let buffer1 = redshirt_hardware_interface::malloc::PhysicalBuffer::new(Packet1 {
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

        assert_eq!(buffer1.pointer() % 16, 0);
        mailbox::write_mailbox(mailbox::Message::new(8, u32::try_from(buffer1.pointer() >> 4).unwrap())).await;      // TODO: ` | 0x40000000` ?*/

    }
}

pub struct PropertyMessageParser<'a> {
    data: Cow<'a, [u32]>,
}

impl<'a> PropertyMessageParser<'a> {
    pub fn from_buffer(buffer: impl Into<Cow<'a, [u32]>>) -> Result<Self, ()> {
        let buffer = buffer.into();
        assert!(buffer.len() >= 2);
        assert_eq!(buffer.len() % 16, 0);
        assert_eq!(u32::try_from(buffer.len()).unwrap(), buffer[0]);
        if buffer[1] != 0x80000000 {
            return Err(())
        }
        Ok(PropertyMessageParser {
            data: buffer,
        })
    }

    /// Returns an iterator to the list of tags of the message.
    pub fn tags<'b: 'a>(&'b self) -> impl Iterator<Item = PropertyMessageParserTag<'a>> + 'b {
        // This is the "state" of our iterator. Corresponds to the start index of the next tag
        // within `self.data`.
        let mut cursor = 2;

        iter::from_fn(move || {
            if cursor >= u32::try_from(self.data.len()).unwrap() {
                return None;
            }

            let value_buffer_size_bytes = self.data[usize::try_from(cursor).unwrap() + 1];
            let value_buffer_num_u32s = value_buffer_size_bytes
                .checked_sub(1)
                .map(|v| 4 * (1 + (v / 4)))
                .unwrap_or(0);

            let tag_start = usize::try_from(cursor).unwrap();
            let tag_total_u32s = 3 + value_buffer_num_u32s;
            cursor += tag_total_u32s;
            let tag_end = usize::try_from(cursor).unwrap();

            Some(PropertyMessageParserTag {
                data: &self.data[tag_start..tag_end],
            })
        })
    }
}

pub struct PropertyMessageParserTag<'a> {
    data: &'a [u32],
}

#[repr(align(16))]
struct Packet1 {
    data: [u32; 20],
}

#[repr(align(16))]
struct Packet2 {
    data: [u32; 8],
}

// TODO: make more generic and explicit, with tags and all, to be more robust to code changes
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

    assert_eq!(buffer1.pointer() % 16, 0);
    mailbox::write_mailbox(mailbox::Message::new(8, u32::try_from(buffer1.pointer() >> 4).unwrap())).await;      // TODO: ` | 0x40000000` ?

    mailbox::read_mailbox().await;

    let data1 = buffer1.take().await;
    assert_eq!(data1.data[1], 0x80000000);

    let actual_width = data1.data[5];
    let actual_height = data1.data[6];

    let buffer2 = redshirt_hardware_interface::malloc::PhysicalBuffer::new(Packet2 {
        data: [
            32,                         // The whole buffer is 32 bytes
            0,                          // This is a request, so the request/response code is 0
            0x00040001, 8, 0, 16, 0,    // This tag requests a 16 byte aligned framebuffer
            0                           // This is the end tag
        ]            
    }).await;

    assert_eq!(buffer2.pointer() % 16, 0);
    mailbox::write_mailbox(mailbox::Message::new(8, u32::try_from(buffer2.pointer() >> 4).unwrap())).await;

    mailbox::read_mailbox().await;

    let data2 = buffer2.take().await;
    assert_eq!(data2.data[1], 0x80000000);

    let fb_addr = data2.data[5];
    let fb_size = data2.data[6];
    //panic!("{:x} size {}", fb_addr, fb_size);

    for x in 0..actual_width {
        for y in 0..actual_height {
            let ptr = fb_addr + 3 * ((y * actual_width) + x);
            let mut op_builder = redshirt_hardware_interface::HardwareWriteOperationsBuilder::new();
            unsafe {
                op_builder.write(u64::from(ptr), vec![0xff, 0xff, 0xff]);
            }
            op_builder.send();

            // TODO: we wait for an answer, otherwise we OOM
            unsafe {
                let mut read = redshirt_hardware_interface::HardwareOperationsBuilder::new();
                let mut out = [0];
                read.read_u32(0x3f000000 + 0xb880 + 0x18, &mut out);
                read.send().await;
            }
        }
    }
}
