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

//! The compositor is responsible for gathering all video outputs, mouse inputs, keyboard inputs,
//! and framebuffers, and mix them together.
//!
//!                       +--------------------------------------------+
//!                       |                                            |
//!                 <-->  |                                            |  +-->
//!                       |                                            |
//!                 <-->  |     +================================>     |  +-->
//!                       |                                            |
//!   framebuffer   <-->  |                                            |  +-->    video output
//!    interface          |                 compositor                 |           interface
//!                 <-->  |                                            |  +-->
//!                       |                                            |
//!                 <-->  |     <================+                     |  +-->
//!                       |                      |                     |
//!                 <-->  |                      +                     |  +-->
//!                       |                                            |
//!                       +--------------------------------------------+
//!
//!                                    ^    ^    ^    ^    ^
//!                                    |    |    |    |    |
//!                                    +    +    +    +    +
//!
//!                                human-input-related interfaces
//!                                (e.g. mouse, keyboard, touch)
//!
//!
//! The compositor considers that there is a *desktop* of infinite dimensions. Framebuffers and
//! video outputs each have an area that overlaps this desktop.

#![no_std]

extern crate alloc;

use alloc::{collections::VecDeque, vec, vec::Vec};
use core::{cmp::Eq, convert::TryFrom as _, hash::Hash, iter, mem, ops::Range};

mod rect;

pub struct Compositor<TFbId, TOutId, TFb, TOut> {
    framebuffers: hashbrown::HashMap<TFbId, Framebuffer<TFb>, ahash::RandomState>,
    video_outputs: hashbrown::HashMap<TOutId, VideoOutput<TOut>, ahash::RandomState>,

    next_framebuffer_position: (u32, u32),
}

struct Framebuffer<TFb> {
    position: rect::Rect,
    user_data: TFb,
    /// Rows of pixels. Each pixel is a RGBA color.
    rgb_data: Vec<[u8; 4]>,
}

struct VideoOutput<TOut> {
    position: rect::Rect,
    format: Format,
    user_data: TOut,
    /// List of areas that need to be refreshed. In local coordinates.
    needs_refresh: VecDeque<rect::Rect>,
}

impl<TFbId: Clone + Eq + Hash, TOutId: Clone + Eq + Hash, TFb, TOut>
    Compositor<TFbId, TOutId, TFb, TOut>
{
    pub fn with_seed(seed: [u8; 64]) -> Self {
        Compositor {
            framebuffers: hashbrown::HashMap::with_capacity_and_hasher(
                256,
                ahash::RandomState::with_seeds(
                    u64::from_ne_bytes(<[u8; 8]>::try_from(&seed[0..8]).unwrap()),
                    u64::from_ne_bytes(<[u8; 8]>::try_from(&seed[8..16]).unwrap()),
                    u64::from_ne_bytes(<[u8; 8]>::try_from(&seed[16..24]).unwrap()),
                    u64::from_ne_bytes(<[u8; 8]>::try_from(&seed[24..32]).unwrap()),
                ),
            ),
            video_outputs: hashbrown::HashMap::with_capacity_and_hasher(
                16,
                ahash::RandomState::with_seeds(
                    u64::from_ne_bytes(<[u8; 8]>::try_from(&seed[32..40]).unwrap()),
                    u64::from_ne_bytes(<[u8; 8]>::try_from(&seed[40..48]).unwrap()),
                    u64::from_ne_bytes(<[u8; 8]>::try_from(&seed[48..56]).unwrap()),
                    u64::from_ne_bytes(<[u8; 8]>::try_from(&seed[56..64]).unwrap()),
                ),
            ),
            next_framebuffer_position: (20, 20),
        }
    }

    pub fn add_video_output(
        &mut self,
        id: TOutId,
        width: u32,
        height: u32,
        format: Format,
        user_data: TOut,
    ) -> VideoOutputAccess<TFbId, TOutId, TFb, TOut> {
        debug_assert!(self.video_outputs.values().any(|out| out.position.x == 0));
        let x_position = self
            .video_outputs
            .values()
            .fold(0, |total, out| total + out.position.width);
        debug_assert!(!self
            .video_outputs
            .values()
            .any(|out| out.position.x + out.position.width > x_position));

        // TODO: error if duplicate
        self.video_outputs.insert(
            id.clone(),
            VideoOutput {
                position: rect::Rect {
                    width,
                    height,
                    x: x_position,
                    y: 0,
                },
                format,
                needs_refresh: {
                    let mut list = VecDeque::with_capacity(16);
                    list.push_back(rect::Rect {
                        x: 0,
                        y: 0,
                        width,
                        height,
                    });
                    list
                },
                user_data,
            },
        );

        VideoOutputAccess { parent: self, id }
    }

    pub fn video_output_by_id(
        &mut self,
        id: &TOutId,
    ) -> Option<VideoOutputAccess<TFbId, TOutId, TFb, TOut>> {
        if self.video_outputs.contains_key(id) {
            Some(VideoOutputAccess {
                parent: self,
                id: id.clone(),
            })
        } else {
            None
        }
    }

    pub fn video_outputs(&self) -> impl ExactSizeIterator<Item = &TOutId> {
        self.video_outputs.keys()
    }

    pub fn add_framebuffer(
        &mut self,
        id: TFbId,
        width: u32,
        height: u32,
        user_data: TFb,
    ) -> FramebufferAccess<TFbId, TOutId, TFb, TOut> {
        let fb_position = rect::Rect {
            width,
            height,
            x: self.next_framebuffer_position.0,
            y: self.next_framebuffer_position.1,
        };

        // TODO: error if duplicate
        self.framebuffers.insert(
            id.clone(),
            Framebuffer {
                position: fb_position,
                user_data,
                // TODO: return error instead of panicking if width*height is too large; there is clearly some attack vector with these width and height values
                rgb_data: (0..usize::try_from(width * height).unwrap())
                    .map(|_| [0; 4])
                    .collect(),
            },
        );

        self.next_framebuffer_position.0 = (self.next_framebuffer_position.0 + 20) % 300;
        self.next_framebuffer_position.1 = (self.next_framebuffer_position.1 + 20) % 200;

        // Invalidate areas from video outputs that overlap with the newly-created framebuffer.
        for video_output in self.video_outputs.values_mut() {
            let overlap = match video_output.position.intersection(&fb_position) {
                Some(ov) => ov,
                None => continue,
            };

            // `overlap` contains desktop positions, while `needs_refresh` contains positions
            // relative to the video output.
            video_output.needs_refresh.push_back(rect::Rect {
                x: overlap.x - video_output.position.x,
                y: overlap.y - video_output.position.y,
                width: overlap.width,
                height: overlap.height,
            });
        }

        FramebufferAccess { parent: self, id }
    }

    pub fn framebuffer_by_id(
        &mut self,
        id: &TFbId,
    ) -> Option<FramebufferAccess<TFbId, TOutId, TFb, TOut>> {
        if self.framebuffers.contains_key(id) {
            Some(FramebufferAccess {
                parent: self,
                id: id.clone(),
            })
        } else {
            None
        }
    }

    pub fn framebuffers(&self) -> impl ExactSizeIterator<Item = &TFbId> {
        self.framebuffers.keys()
    }

    /// Updates the state machine after one frame has passed.
    pub fn next_frame(&mut self) {
        // TODO: is this necessary? consider removing if this does nothing
    }

    /// Finds the color of the pixel at the given desktop coordinates.
    fn desktop_pixel(&self, x: u32, y: u32) -> [u8; 3] {
        // TODO: this method is probably naive and super slow
        // TODO: properly handle z layers

        let mut accumulator = [255, 255, 255];

        for framebuffer in self.framebuffers.values() {
            let fb_offset_x = match x.checked_sub(framebuffer.position.x) {
                Some(off) => off,
                None => continue,
            };

            let fb_offset_y = match y.checked_sub(framebuffer.position.y) {
                Some(off) => off,
                None => continue,
            };

            if fb_offset_x >= framebuffer.position.width {
                continue;
            }

            if fb_offset_y >= framebuffer.position.height {
                continue;
            }

            let fb_pixel = framebuffer.rgb_data
                [usize::try_from(fb_offset_y * framebuffer.position.width + fb_offset_x).unwrap()];
            accumulator = blend(fb_pixel, accumulator);
        }

        accumulator
    }
}

fn blend(a: [u8; 4], b: [u8; 3]) -> [u8; 3] {
    let b_alpha = u16::from(255 - a[3]);

    let r = u16::from(a[0]) * u16::from(a[3]) + u16::from(b[0]) * b_alpha;
    let g = u16::from(a[2]) * u16::from(a[3]) + u16::from(b[1]) * b_alpha;
    let b = u16::from(a[1]) * u16::from(a[3]) + u16::from(b[2]) * b_alpha;

    [
        u8::try_from(r / 255).unwrap(),
        u8::try_from(g / 255).unwrap(),
        u8::try_from(b / 255).unwrap(),
    ]
}

/// Access to a framebuffer within a [`Compositor`].
pub struct FramebufferAccess<'a, TFbId, TOutId, TFb, TOut> {
    parent: &'a mut Compositor<TFbId, TOutId, TFb, TOut>,
    id: TFbId,
}

impl<'a, TFbId: Clone + Eq + Hash, TOutId: Clone + Eq + Hash, TFb, TOut>
    FramebufferAccess<'a, TFbId, TOutId, TFb, TOut>
{
    /// Removes the framebuffer from the compositor state machine.
    pub fn remove(self) -> TFb {
        self.parent.framebuffers.remove(&self.id).unwrap().user_data
    }

    pub fn user_data(&self) -> &TFb {
        &self.parent.framebuffers.get(&self.id).unwrap().user_data
    }

    pub fn user_data_mut(&mut self) -> &mut TFb {
        &mut self
            .parent
            .framebuffers
            .get_mut(&self.id)
            .unwrap()
            .user_data
    }

    /// Sets the content of the framebuffer.
    ///
    /// This potentially pushes pending changes to the various video outputs that can later be
    /// retreived using [`VideoOutputAccess::drain_pending_changes`].
    pub fn set_content(&mut self, x_range: Range<u32>, y_range: Range<u32>, data: &[u8]) {}
}

/// Access to a video output within a [`Compositor`].
pub struct VideoOutputAccess<'a, TFbId, TOutId, TFb, TOut> {
    parent: &'a mut Compositor<TFbId, TOutId, TFb, TOut>,
    id: TOutId,
}

impl<'a, TFbId: Clone + Eq + Hash, TOutId: Clone + Eq + Hash, TFb, TOut>
    VideoOutputAccess<'a, TFbId, TOutId, TFb, TOut>
{
    /// Removes the video output from the compositor state machine.
    pub fn remove(self) -> TOut {
        self.parent
            .video_outputs
            .remove(&self.id)
            .unwrap()
            .user_data
    }

    pub fn user_data(&self) -> &TOut {
        &self.parent.video_outputs.get(&self.id).unwrap().user_data
    }

    pub fn user_data_mut(&mut self) -> &mut TOut {
        &mut self
            .parent
            .video_outputs
            .get_mut(&self.id)
            .unwrap()
            .user_data
    }

    pub fn drain_pending_changes<'b: 'a>(&'b mut self) -> impl Iterator<Item = PendingChange> + 'b {
        iter::from_fn(move || {
            let video_output = self.parent.video_outputs.get_mut(&self.id).unwrap();
            let area = video_output.needs_refresh.pop_front()?;
            let video_output_position = video_output.position;
            let video_output_format = video_output.format;

            Some(PendingChange {
                screen_x_start: area.x,
                screen_x_len: area.width,
                screen_y_start: area.y,
                pixels: (area.y..area.y + area.height)
                    .map(|y| {
                        let desktop_y = y + video_output_position.y;
                        (area.x..area.x + area.width)
                            .flat_map(|x| {
                                let desktop_x = x + video_output_position.x;
                                let pixel = self.parent.desktop_pixel(desktop_x, desktop_y);
                                convert_format(pixel, &video_output_format)
                            })
                            .collect()
                    })
                    .collect(),
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct PendingChange {
    pub screen_x_start: u32,
    // TODO: not necessary?
    pub screen_x_len: u32,
    pub screen_y_start: u32,
    /// Rows of pixels.
    pub pixels: Vec<Vec<u8>>,
}

#[derive(Debug, Copy, Clone)]
pub enum Format {
    R8G8B8X8,
}

fn convert_format(pixel: [u8; 3], format: &Format) -> impl Iterator<Item = u8> {
    match format {
        Format::R8G8B8X8 => iter::once(pixel[0])
            .chain(iter::once(pixel[1]))
            .chain(iter::once(pixel[2]))
            .chain(iter::once(0xff)),
    }
}
