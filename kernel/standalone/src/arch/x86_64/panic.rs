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

//! Panic handling code.

use alloc::sync::Arc;
use spinning_top::Spinlock;

/// Modifies the framebuffer information. Used when printing a panic.
pub fn set_framebuffer_info(info: FramebufferInfo) {
    *FB_INFO.lock() = info;
}

static PANIC_LOGGER: Spinlock<Option<Arc<KLogger>>> = Spinlock::new(None);

#[cfg(not(any(test, doc, doctest)))]
#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    let info = FB_INFO.lock();

    let mut printer = Printer::from(&*info);
    let _ = writeln!(printer, "Kernel panic!");
    let _ = writeln!(printer, "{}", panic_info);
    let _ = writeln!(printer, "");

    // Freeze forever.
    loop {
        x86_64::instructions::interrupts::disable();
        x86_64::instructions::hlt();
    }
}

struct Printer<'a> {
    info: &'a FramebufferInfo,
    cursor_x: u32,
    cursor_y: u32,
    character_width: u32,
    character_height: u32,
}

impl<'a> From<&'a FramebufferInfo> for Printer<'a> {
    fn from(info: &'a FramebufferInfo) -> Self {
        let (character_width, character_height) = match info.format {
            FramebufferFormat::Text => (1, 1),
            FramebufferFormat::Rgb { .. } => {
                // TODO: yeah, no
                (info.width / 100, info.height / 25)
            }
        };

        Printer {
            info,
            cursor_x: 0,
            cursor_y: 0,
            character_width,
            character_height,
        }
    }
}

impl<'a> fmt::Write for Printer<'a> {
    fn write_str(&mut self, message: &str) -> fmt::Result {
        for chr in message.chars() {
            if !chr.is_ascii() {
                continue;
            }
            // TODO: better way to convert to ASCII?
            let chr = chr as u8;

            // We assume that panic messages are never more than the height of the screen
            // and discard everything after.
            if self.cursor_y >= self.info.height {
                break;
            }

            if chr == b'\n' {
                self.carriage_return();
                continue;
            }

            self.print_at_cursor(chr);

            debug_assert!(self.cursor_x < self.info.width);
            self.cursor_x = self.cursor_x.saturating_add(self.character_width);
            if self.cursor_x > self.info.width.saturating_sub(self.character_width) {
                self.carriage_return();
            }
        }

        Ok(())
    }
}

impl<'a> Printer<'a> {
    fn print_at_cursor(&mut self, chr: u8) {
        unsafe {
            let y_offset = self
                .info
                .pitch
                .saturating_mul(usize::try_from(self.cursor_y).unwrap_or(usize::max_value()));
            let x_offset = usize::try_from(self.cursor_x)
                .unwrap_or(usize::max_value())
                .saturating_mul(self.info.bpp);
            let addr = (self.info.address as *mut u8).add(x_offset.saturating_add(y_offset));

            match self.info.format {
                FramebufferFormat::Text => {
                    (addr as *mut u16).write_volatile(u16::from(chr) | 0xc00);
                }
                FramebufferFormat::Rgb => {
                    for x in (self.cursor_x as usize)..((self.cursor_x+self.character_width) as usize) {
                        for y in (self.cursor_y as usize)..((self.cursor_y+self.character_height) as usize) {
                            let px_addr = addr.add(x * self.info.bpp).add(y * self.info.pitch);
                            for offset in 0..self.info.bpp {
                                *px_addr.add(offset) = 0xff;
                            }
                        }
                    }
                }
            }
        }
    }

    fn carriage_return(&mut self) {
        self.cursor_x = 0;
        self.cursor_y = self.cursor_y.saturating_add(self.character_height);
        if !matches!(self.info.format, FramebufferFormat::Text) {
            // Some padding.
            self.cursor_y = self.cursor_y.saturating_add(4);
        }
    }
}
