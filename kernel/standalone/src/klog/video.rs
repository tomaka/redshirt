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

use core::{convert::TryFrom as _, fmt, mem::MaybeUninit, ptr};
use redshirt_kernel_log_interface::ffi::{FramebufferFormat, FramebufferInfo};

pub struct Terminal {
    framebuffer: FramebufferInfo,
    cursor_x: u32,
    cursor_y: u32,
    character_width: u32,
    character_height: u32,
}

impl Terminal {
    /// Clears the given framebuffer and returns a terminal.
    pub const unsafe fn new(framebuffer: FramebufferInfo) -> Terminal {
        // TODO: proper calculation based on screen dimensions
        let character_dims = match framebuffer.format {
            FramebufferFormat::Text => 1,
            FramebufferFormat::Rgb { .. } => 8,
        };

        Terminal {
            framebuffer,
            cursor_x: 0,
            cursor_y: 0,
            character_width: character_dims,
            character_height: character_dims,
        }
    }

    /// Clears the framebuffer with black.
    pub fn clear_screen(&mut self) {
        // Safety is covered by `Terminal::new`.
        unsafe {
            clear_screen(&self.framebuffer);
            self.cursor_x = 0;
            self.cursor_y = 0;
        }
    }

    /// Returns an object that implements `core::fmt::Write` designed for printing a message.
    pub fn printer<'a>(&'a mut self, color: [u8; 3]) -> impl fmt::Write + 'a {
        struct Printer<'a> {
            klog: &'a mut Terminal,
            color: [u8; 3],
        }
        impl<'a> fmt::Write for Printer<'a> {
            fn write_str(&mut self, message: &str) -> fmt::Result {
                self.klog.print(message, self.color);
                Ok(())
            }
        }
        Printer { klog: self, color }
    }

    /// Adds a message to the terminal.
    fn print(&mut self, message: &str, color: [u8; 3]) {
        for chr in message.chars() {
            if !chr.is_ascii() {
                continue;
            }
            // TODO: better way to convert to ASCII?
            let chr = chr as u8;

            if chr == b'\n' {
                self.carriage_return();
                continue;
            }

            self.print_at_cursor(chr, color);

            debug_assert!(self.cursor_x < self.framebuffer.width);
            self.cursor_x = self.cursor_x.saturating_add(self.character_width);
            if self.cursor_x > self.framebuffer.width.saturating_sub(self.character_width) {
                self.carriage_return();
            }
        }
    }

    /// Returns the memory address where the cursor is currently located.
    fn cursor_mem_address(&self) -> *mut u8 {
        unsafe {
            let y_offset = usize::try_from(
                self.framebuffer
                    .pitch
                    .saturating_mul(u64::from(self.cursor_y)),
            )
            .unwrap_or(usize::max_value());
            let x_offset = usize::try_from(self.cursor_x)
                .unwrap_or(usize::max_value())
                .saturating_mul(usize::from(self.framebuffer.bytes_per_character));
            (self.framebuffer.address as *mut u8).add(x_offset.saturating_add(y_offset))
        }
    }

    fn print_at_cursor(&mut self, chr: u8, color: [u8; 3]) {
        unsafe {
            let addr = self.cursor_mem_address();

            match self.framebuffer.format {
                FramebufferFormat::Text => {
                    // TODO: proper color
                    (addr as *mut u16).write_volatile(u16::from(chr) | 0xc00);
                }
                FramebufferFormat::Rgb { .. } => {
                    let src_data = {
                        let idx = usize::from(chr % 128) * 64;
                        &FONT_DATA[idx..idx + 64]
                    };

                    let bpp = usize::from(self.framebuffer.bytes_per_character);
                    let chr_width = match usize::try_from(self.character_width) {
                        Ok(w) => w,
                        _ => return,
                    };
                    let chr_height = match usize::try_from(self.character_height) {
                        Ok(h) => h,
                        _ => return,
                    };
                    let pitch = match usize::try_from(self.framebuffer.pitch) {
                        Ok(p) => p,
                        _ => return,
                    };

                    for y in 0..chr_height {
                        let addr = addr.add(y.saturating_mul(pitch));

                        for x in 0..chr_width {
                            // FIXME: only works because we hard-code 8 as character width/height
                            // should be properly sampled
                            let src_px = src_data[y * 8 + x];
                            let r = mix(src_px, color[0]);
                            let g = mix(src_px, color[1]);
                            let b = mix(src_px, color[2]);

                            let addr = addr.add(x.saturating_mul(bpp));
                            write_rgb_color(addr, &self.framebuffer, [r, g, b]);
                        }
                    }
                }
            }
        }
    }

    /// Moves the cursor and the content of the screen one line up.
    fn line_down(&mut self) {
        // Safety is covered by `Terminal::new`.
        unsafe {
            // Note that we read from the framebuffer here, which is extremely slow, but is also the
            // only way we can avoid memory allocations.
            copy_row_up(&self.framebuffer, self.character_height);
            self.cursor_y = self.cursor_y.saturating_sub(self.character_height);
        }
    }

    fn carriage_return(&mut self) {
        self.cursor_x = 0;
        self.cursor_y = self.cursor_y.saturating_add(self.character_height);
        if !matches!(self.framebuffer.format, FramebufferFormat::Text) {
            // Some padding.
            // TODO: implement better
            self.cursor_y = self.cursor_y.saturating_add(4);
        }
        while self.cursor_y
            >= self
                .framebuffer
                .height
                .saturating_add(self.character_height)
        {
            self.line_down();
        }
    }
}

/// Font data generated by a build script.
///
/// Contains the 128 ASCII characters. Each character is 8x8 bytes. Each byte contains the
/// opacity of the corresponding pixel, where `0x0` means transparent and `0xff` means opaque.
const FONT_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/font.bin"));

fn mix(v1: u8, v2: u8) -> u8 {
    ((u16::from(v1) * u16::from(v2)) / 255) as u8
}

/// Copies all the rows of the framebuffer `n` rows up and clears the last `n` rows.
unsafe fn copy_row_up(info: &FramebufferInfo, rows_up: u32) {
    let ptr = match usize::try_from(info.address) {
        Ok(p) => p as *mut u8,
        _ => return,
    };

    let width = match usize::try_from(info.width) {
        Ok(w) => w,
        _ => return,
    };

    let height = match usize::try_from(info.height) {
        Ok(h) => h,
        _ => return,
    };

    let rows_up = match usize::try_from(rows_up) {
        Ok(0) => return,
        Ok(r) => r,
        _ => return,
    };

    let pitch = match usize::try_from(info.pitch) {
        Ok(p) => p,
        _ => return,
    };

    let bpp = usize::from(info.bytes_per_character);

    for y in 0..(height - rows_up) {
        let src = ptr.add(pitch.saturating_mul(y + rows_up));
        let dst = ptr.add(pitch.saturating_mul(y));
        // Note: we don't use `copy_non_overlapping` as we don't actually know if that's true.
        ptr::copy(src, dst, bpp.saturating_mul(width));
    }

    for y in (height - rows_up)..height {
        let dst = ptr.add(pitch.saturating_mul(y));
        ptr::write_bytes(dst, 0x0, bpp.saturating_mul(width));
    }
}

/// Writes `0x0` on the entire framebuffer.
unsafe fn clear_screen(info: &FramebufferInfo) {
    let ptr = match usize::try_from(info.address) {
        Ok(p) => p as *mut u8,
        _ => return,
    };

    let width = match usize::try_from(info.width) {
        Ok(w) => w,
        _ => return,
    };

    let height = match usize::try_from(info.height) {
        Ok(h) => h,
        _ => return,
    };

    let pitch = match usize::try_from(info.pitch) {
        Ok(p) => p,
        _ => return,
    };

    let bpp = usize::from(info.bytes_per_character);

    for y in 0..height {
        let ptr = ptr.add(pitch.saturating_mul(y));
        ptr::write_bytes(ptr, 0x0, bpp.saturating_mul(width));
    }
}

unsafe fn write_rgb_color(dst: *mut u8, info: &FramebufferInfo, color: [u8; 3]) {
    if info.bytes_per_character > 4 {
        // TODO: not supported
        return;
    }

    let mut pixel_to_write = 0u32;

    if let FramebufferFormat::Rgb {
        red_size,
        red_position,
        green_size,
        green_position,
        blue_size,
        blue_position,
    } = info.format
    {
        let red = if red_size >= 8 {
            u32::from(color[0]).wrapping_shl(u32::from(red_size) - 8)
        } else {
            u32::from(color[0]).wrapping_shr(8 - u32::from(red_size))
        };

        let green = if green_size >= 8 {
            u32::from(color[1]).wrapping_shl(u32::from(green_size) - 8)
        } else {
            u32::from(color[1]).wrapping_shr(8 - u32::from(green_size))
        };

        let blue = if blue_size >= 8 {
            u32::from(color[2]).wrapping_shl(u32::from(blue_size) - 8)
        } else {
            u32::from(color[2]).wrapping_shr(8 - u32::from(blue_size))
        };

        pixel_to_write = red.wrapping_shl(u32::from(red_position))
            | green.wrapping_shl(u32::from(green_position))
            | blue.wrapping_shl(u32::from(blue_position));
    } // TODO: else?

    let pixel_to_write = pixel_to_write.to_le_bytes(); // TODO: LE bytes? lol, this one is hard
    ptr::copy_nonoverlapping(
        pixel_to_write.as_ptr(),
        dst,
        usize::from(info.bytes_per_character),
    );
}
