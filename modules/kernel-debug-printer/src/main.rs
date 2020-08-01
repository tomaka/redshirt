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

use redshirt_time_interface::Delay;
use std::time::Duration;

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    loop {
        let metrics = redshirt_kernel_debug_interface::get_metrics().await;
        print!("{}", metrics);

        // Wait 5 seconds for next print.
        Delay::new(Duration::from_secs(5)).await;
    }
}