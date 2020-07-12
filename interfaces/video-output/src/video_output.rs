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

//! Registering video outputs.
//!
//! Use this if you're writing for example a video card driver.

use crate::ffi;
use core::fmt;
use futures::{lock::Mutex, prelude::*};
use redshirt_syscalls::Encode as _;

/// Configuration of a video output to register.
#[derive(Debug)]
pub struct VideoOutputConfig {
    /// Width in pixels of the output.
    pub width: u32,
    /// Height in pixels of the output.
    pub height: u32,
    /// Format of the output.
    pub format: ffi::Format,
}

/// Registers a new video output.
pub async fn register(config: VideoOutputConfig) -> VideoOutputRegistration {
    unsafe {
        let id = redshirt_random_interface::generate_u64().await;

        redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &{
            ffi::VideoOutputMessage::Register {
                id,
                width: config.width,
                height: config.height,
                format: config.format,
            }
        })
        .unwrap();

        VideoOutputRegistration {
            id,
            frames: Mutex::new((0..10).map(|_| build_frame_future(id)).collect()),
        }
    }
}

/// Registered network interface.
///
/// Destroying this object will unregister the interface.
pub struct VideoOutputRegistration {
    /// Identifier of the interface in the network manager.
    id: u64,
    /// Futures that resolve when we receive the next frame to draw.
    frames: Mutex<stream::FuturesOrdered<redshirt_syscalls::MessageResponseFuture<ffi::NextImage>>>,
}

/// Build a `Future` resolving to the next frame to display.
fn build_frame_future(id: u64) -> redshirt_syscalls::MessageResponseFuture<ffi::NextImage> {
    unsafe {
        let message = ffi::VideoOutputMessage::NextImage(id).encode();
        let msg_id = redshirt_syscalls::MessageBuilder::new()
            .add_data(&message)
            .emit_with_response_raw(&ffi::INTERFACE)
            .unwrap();
        redshirt_syscalls::message_response(msg_id)
    }
}

impl VideoOutputRegistration {
    /// Returns the next packet to send to the network.
    ///
    /// This function will pull and merge all the pending frames into one. Even if the code calling
    /// this method lags behind, only one frame will be returned.
    ///
    /// > **Note**: It is possible to call this method multiple times on the same
    /// >           [`VideoOutputRegistration`]. If that is done, no guarantee exists as to which
    /// >           `Future` finishes first.
    pub async fn next_frame(&self) -> ffi::NextImage {
        let mut frames = self.frames.lock().await;

        let mut out = frames.next().await.unwrap();
        while let Some(next_frame) = frames.next().now_or_never() {
            let next_frame = next_frame.unwrap();
            out.changes.extend(next_frame.changes);
        }
        out
    }
}

impl fmt::Debug for VideoOutputRegistration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("VideoOutputRegistration")
            .field(&self.id)
            .finish()
    }
}

impl Drop for VideoOutputRegistration {
    fn drop(&mut self) {
        unsafe {
            let message = ffi::VideoOutputMessage::Unregister(self.id);
            redshirt_syscalls::emit_message_without_response(&ffi::INTERFACE, &message).unwrap();
        }
    }
}
