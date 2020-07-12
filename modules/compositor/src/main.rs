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

#![recursion_limit = "2048"]

use futures::prelude::*;
use redshirt_syscalls::{ffi::DecodedInterfaceOrDestroyed, Decode as _, MessageId, Pid};
use redshirt_time_interface::Delay;
use redshirt_video_output_interface::ffi as vid_ffi;
use std::{collections::VecDeque, time::Duration};

fn main() {
    redshirt_syscalls::block_on(async_main())
}

async fn async_main() {
    // Register the interfaces.
    redshirt_interface_interface::register_interface(vid_ffi::INTERFACE)
        .await
        .unwrap();

    struct VideoOutput {
        pid: Pid,
        id: u64,
        cleared: bool,
        width: u32,
        height: u32,
        format: vid_ffi::Format,
        next_frame_messages: VecDeque<MessageId>,
    }

    let mut video_outputs = Vec::<VideoOutput>::new();
    let mut next_frame = Delay::new(Duration::from_secs(5)).fuse(); // TODO:

    loop {
        futures::select! {
            interface_event = redshirt_syscalls::next_interface_message().fuse() => {
                match interface_event {
                    DecodedInterfaceOrDestroyed::Interface(msg) => {
                        if msg.interface == vid_ffi::INTERFACE {
                            let emitter_pid = msg.emitter_pid;
                            let msg_data = vid_ffi::VideoOutputMessage::decode(msg.actual_data).unwrap();
                            match msg_data {
                                vid_ffi::VideoOutputMessage::Register { id, width, height, format } => {
                                    video_outputs.push(VideoOutput {
                                        pid: msg.emitter_pid,
                                        id,
                                        cleared: false,
                                        width,
                                        height,
                                        format,
                                        next_frame_messages: VecDeque::with_capacity(16),
                                    });
                                }
                                vid_ffi::VideoOutputMessage::Unregister(id) => {
                                    video_outputs.retain(|vo| vo.pid != emitter_pid || vo.id != id);
                                }
                                vid_ffi::VideoOutputMessage::NextImage(id) => {
                                    if let Some(message_id) = msg.message_id {
                                        if let Some(vo) = video_outputs.iter_mut().find(|vo| vo.pid == emitter_pid && vo.id == id) {
                                            vo.next_frame_messages.push_back(message_id)
                                        } else {
                                            redshirt_syscalls::emit_message_error(message_id);
                                        }
                                    }
                                }
                            }
                        } else {
                            unreachable!()
                        }
                    },
                    DecodedInterfaceOrDestroyed::ProcessDestroyed(destroyed) => {
                        video_outputs.retain(|vo| vo.pid != destroyed.pid);
                    }
                }
            },
            _ = next_frame => {
                println!("frame!");
                next_frame = Delay::new(Duration::from_secs(1)).fuse();        // TODO:
                for video_output in &mut video_outputs {
                    let message_id = match video_output.next_frame_messages.pop_front() {
                        Some(m) => m,
                        None => continue,
                    };

                    redshirt_syscalls::emit_answer(message_id, vid_ffi::NextImage {
                        changes: if video_output.cleared {
                            Vec::new()
                        } else {
                            vec![vid_ffi::NextImageChange {
                                screen_x_start: 0,
                                screen_x_len: video_output.width,
                                screen_y_start: 0,
                                pixels: (0..video_output.height).map(|_| {
                                    (0..video_output.width * 3).map(|_| 0xffu8).collect::<Vec<_>>()
                                }).collect(),
                            }]
                        },
                    });
                }
            }
        }
    }
}
