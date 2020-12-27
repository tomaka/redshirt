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

use std::{convert::TryFrom as _, env, fs, path::Path};

fn main() {
    let font_data = gen_font();
    assert_eq!(font_data.len(), 128 * 8 * 8);

    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(Path::new(&out_dir).join("font.bin"), &font_data).unwrap();
}

/// Generates a font sprite sheet of the 128 ASCII characters.
///
/// Each character is 8x8 pixel. Each pixel is a single byte indicating its opacity. A value
/// of 0x0 means transparent, and a value of 0xff means opaque.
///
/// In other words, the returned data is 128 * 8 * 8 bytes.
fn gen_font() -> Vec<u8> {
    let font_data: &[u8] = include_bytes!("vcr_osd_mono.ttf");
    let font = rusttype::Font::try_from_bytes(font_data).unwrap();

    let mut out_data = vec![0; 128 * 8 * 8];
    for ascii_chr in 0..128u8 {
        let glyph = font
            .glyph(char::from(ascii_chr))
            .scaled(rusttype::Scale { x: 8.0, y: 8.0 })
            .positioned(rusttype::Point { x: 0.0, y: 0.0 });

        // `pixel_bound_box` returns `None` for glyphs that are empty (like the space character)
        let bbox = match glyph.pixel_bounding_box() {
            Some(b) => b,
            None => continue,
        };

        glyph.draw(|x, y, value| {
            let x = i32::try_from(x).unwrap() + bbox.min.x;
            let y = 8 + i32::try_from(y).unwrap() + bbox.min.y;
            if x < 0 || x >= 8 || y < 0 || y >= 8 {
                return;
            }

            assert!(value >= 0.0 && value <= 1.0);
            let value = (value * 255.0) as u8;
            let b_pos = usize::from(ascii_chr) * 8 * 8 + usize::try_from(x + y * 8).unwrap();
            out_data[b_pos] = value;
        });
    }

    out_data
}
