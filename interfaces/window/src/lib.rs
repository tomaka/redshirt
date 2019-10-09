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

//! Access to the windowing manager.

#![deny(intra_doc_link_resolution_failure)]

// TODO: everything here is a draft

pub mod ffi;

pub struct Window {
    handle: u64,
}

impl Window {
    // TODO: unsafe? what's the lifetime of the window compared to the instance?
    pub async fn open(vk_instance: usize) -> Result<Window, ()> {
        // TODO: properly check that instance fits in u32; this is the vulkan hack where we translate dispatchable handles
        let open = ffi::WindowMessage::Open(ffi::WindowOpen { vk_instance: vk_instance as u32 });
        let response: ffi::WindowOpenResponse =
            nametbd_syscalls_interface::emit_message_with_response(ffi::INTERFACE, open).await?;
        Ok(Window {
            handle: response.result?,
        })
    }

    pub fn as_vulkan_surface(&self) -> u64 {
        self.handle
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let close = ffi::WindowMessage::Close(ffi::WindowClose {
            window_id: self.handle,
        });

        let _ = nametbd_syscalls_interface::emit_message(&ffi::INTERFACE, &close, false);
    }
}
