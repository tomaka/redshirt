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

use std::time::Duration;

fn main() {
    redshirt_syscalls::block_on(async_main())
}

async fn async_main() {
    let fb = redshirt_framebuffer_interface::Framebuffer::new(800, 600).await;
    let mut display = desktop::Desktop::new([800, 600]);

    let mut next_draw = redshirt_time_interface::monotonic_wait(Duration::from_millis(0));

    loop {
        (&mut next_draw).await;
        next_draw = redshirt_time_interface::monotonic_wait(Duration::from_millis(100));

        display.render();
        fb.set_data(display.pixels());
    }
}
