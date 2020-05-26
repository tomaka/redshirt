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

//! Desktop environment.

use futures::prelude::*;
use redshirt_framebuffer_interface::ffi;

fn main() {
    redshirt_syscalls::block_on(async_main())
}

async fn async_main() {
    let mut fb = redshirt_framebuffer_interface::Framebuffer::new(true, 800, 600).await;
    let mut display = desktop::Desktop::new([800, 600]).await;

    loop {
        display.render();
        fb.set_data(display.pixels());

        let events = {
            let mut events = vec![fb.next_event().await];
            // TODO: use now_or_never after https://github.com/tomaka/redshirt/issues/447 is fixed
            loop {
                let ev = fb.next_event();
                futures::pin_mut!(ev);
                match future::select(ev, future::ready(())).await {
                    future::Either::Left((ev, _)) => events.push(ev),
                    future::Either::Right(((), _)) => break,
                }
            }
            events
        };
        println!("received events: {:?}", events);

        for event in events {
            match event {
                ffi::Event::CursorMoved { new_position: Some(pos) } => {
                    display.set_cursor_position(Some([pos.0 as f32 / 1000.0, pos.1 as f32 / 1000.0]));
                }
                ffi::Event::CursorMoved { new_position: None } => {
                    display.set_cursor_position(None);
                }
                _ => {}
            }
        }
    }
}
