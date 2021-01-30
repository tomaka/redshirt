// Copyright (C) 2019-2021  Pierre Krieger
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
use rand::RngCore as _;
use redshirt_framebuffer_interface::ffi as fb_ffi;
use redshirt_interface_interface::DecodedInterfaceOrDestroyed;
use redshirt_syscalls::{Decode as _, MessageId, Pid};
use redshirt_time_interface::Delay;
use redshirt_video_output_interface::ffi as vid_ffi;
use std::{collections::VecDeque, convert::TryFrom as _, time::Duration};

fn main() {
    redshirt_syscalls::block_on(async_main())
}

async fn async_main() {
    // Register the interfaces.
    let mut video_registration =
        redshirt_interface_interface::register_interface(vid_ffi::INTERFACE)
            .await
            .unwrap();
    let mut framebuffer_events_registration =
        redshirt_interface_interface::register_interface(fb_ffi::INTERFACE_WITH_EVENTS)
            .await
            .unwrap();
    let mut framebuffer_noevents_registration =
        redshirt_interface_interface::register_interface(fb_ffi::INTERFACE_WITHOUT_EVENTS)
            .await
            .unwrap();

    // Main state machine holding all the information used below.
    let mut compositor = compositor::Compositor::with_seed({
        let mut seed = [0; 64];
        rand::thread_rng().fill_bytes(&mut seed);
        seed
    });

    struct VideoOutput {
        next_frame_messages: VecDeque<MessageId>,
    }

    struct Framebuffer {
        next_event_messages: VecDeque<MessageId>,
    }

    let mut next_frame = Delay::new(Duration::from_secs(0)).fuse();

    loop {
        futures::select! {
            video_output_event = video_registration.next_message_raw().fuse() => {
                match video_output_event {
                    DecodedInterfaceOrDestroyed::Interface(msg) => {
                        match vid_ffi::VideoOutputMessage::decode(msg.actual_data).unwrap() {
                            vid_ffi::VideoOutputMessage::Register { id, width, height, format } => {
                                let format = match format {
                                    vid_ffi::Format::R8G8B8X8 => compositor::Format::R8G8B8X8,
                                };

                                compositor.add_video_output((msg.emitter_pid, id), width, height, format, VideoOutput {
                                    next_frame_messages: VecDeque::with_capacity(16),
                                });
                            }
                            vid_ffi::VideoOutputMessage::Unregister(id) => {
                                if let Some(vo) = compositor.video_output_by_id(&(msg.emitter_pid, id)) {
                                    let video_output = vo.remove();
                                    for message_id in video_output.next_frame_messages {
                                        redshirt_interface_interface::emit_message_error(message_id);
                                    }
                                }
                            }
                            vid_ffi::VideoOutputMessage::NextImage(id) => {
                                if let Some(message_id) = msg.message_id {
                                    if let Some(mut vo) = compositor.video_output_by_id(&(msg.emitter_pid, id)) {
                                        // TODO: add some limit to the number of events
                                        vo.user_data_mut().next_frame_messages.push_back(message_id)
                                    } else {
                                        redshirt_interface_interface::emit_message_error(message_id)
                                    }
                                }
                            }
                        }
                    },
                    DecodedInterfaceOrDestroyed::ProcessDestroyed(destroyed) => {
                        for video_output_id in compositor.video_outputs().cloned().collect::<Vec<_>>() {
                            if video_output_id.0 != destroyed.pid {
                                continue;
                            }

                            compositor.video_output_by_id(&video_output_id).unwrap().remove();
                        }
                    }
                }
            },

            framebuffer_event = framebuffer_events_registration.next_message_raw().fuse() => {
                match framebuffer_event {
                    DecodedInterfaceOrDestroyed::Interface(msg) => {
                        match msg.actual_data.0.get(0) {
                            Some(0) if msg.actual_data.0.len() == 13 => {
                                let fb_id = u32::from_le_bytes(<[u8; 4]>::try_from(&msg.actual_data.0[1..5]).unwrap());
                                let width = u32::from_le_bytes(<[u8; 4]>::try_from(&msg.actual_data.0[5..9]).unwrap());
                                let height = u32::from_le_bytes(<[u8; 4]>::try_from(&msg.actual_data.0[9..13]).unwrap());
                                compositor.add_framebuffer((msg.emitter_pid, fb_id), width, height, Framebuffer {
                                    next_event_messages: VecDeque::with_capacity(16),
                                });
                            }
                            Some(1) if msg.actual_data.0.len() == 5 => {
                                let fb_id = u32::from_le_bytes(<[u8; 4]>::try_from(&msg.actual_data.0[1..5]).unwrap());
                                if let Some(fb) = compositor.framebuffer_by_id(&(msg.emitter_pid, fb_id)) {
                                    let framebuffer = fb.remove();
                                    for message_id in framebuffer.next_event_messages {
                                        redshirt_interface_interface::emit_message_error(message_id);
                                    }
                                }
                            }
                            // TODO: Some(2) handling
                            Some(3) if msg.actual_data.0.len() == 5 => {
                                let fb_id = u32::from_le_bytes(<[u8; 4]>::try_from(&msg.actual_data.0[1..5]).unwrap());
                                if let Some(message_id) = msg.message_id {
                                    if let Some(mut fb) = compositor.framebuffer_by_id(&(msg.emitter_pid, fb_id)) {
                                        // TODO: add some limit to the number of events
                                        fb.user_data_mut().next_event_messages.push_back(message_id);
                                    } else {
                                        redshirt_interface_interface::emit_message_error(message_id);
                                    }
                                }
                            }
                            _ => {
                                if let Some(message_id) = msg.message_id {
                                    redshirt_interface_interface::emit_message_error(message_id);
                                }
                            }
                        }
                    },
                    DecodedInterfaceOrDestroyed::ProcessDestroyed(destroyed) => {
                        for framebuffer_id in compositor.framebuffers().cloned().collect::<Vec<_>>() {
                            if framebuffer_id.0 != destroyed.pid {
                                continue;
                            }

                            compositor.framebuffer_by_id(&framebuffer_id).unwrap().remove();
                        }
                    }
                }
            },

            () = next_frame => {
                compositor.next_frame();
                next_frame = Delay::new(Duration::new(0, 16666667)).fuse();
                for video_output_id in compositor.video_outputs().cloned().collect::<Vec<_>>() {
                    let mut video_output = compositor.video_output_by_id(&video_output_id).unwrap();

                    let message_id = match video_output.user_data_mut().next_frame_messages.pop_front() {
                        Some(m) => m,
                        None => continue,
                    };

                    redshirt_interface_interface::emit_answer(message_id, vid_ffi::NextImage {
                        changes: video_output.drain_pending_changes().map(|change| {
                            vid_ffi::NextImageChange {
                                screen_x_start: change.screen_x_start,
                                screen_x_len: change.screen_x_len,
                                screen_y_start: change.screen_y_start,
                                pixels: change.pixels,
                            }
                        }).collect(),
                    });
                }
            }
        }
    }
}
