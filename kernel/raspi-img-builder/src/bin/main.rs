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

use std::{fs, path::PathBuf};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "redshirt-raspi-img-builder",
    about = "Redshirt Raspberry PI image builder."
)]
struct CliOptions {
    /// Target file to write.
    #[structopt(long, short, parse(from_os_str))]
    out: PathBuf,
}

fn main() {
    let cli_opts = CliOptions::from_args();

    let img_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(cli_opts.out)
        .expect("Failed to open output file");
    img_file.set_len(4 * 1024 * 1024 * 1024).unwrap();
    let img_file = fscommon::BufStream::new(img_file);
    raspi_img_builder::generate(img_file)
        .expect("Failed to generate file");
}
