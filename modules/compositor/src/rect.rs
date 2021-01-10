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

use core::cmp;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Returns the intersection between this rectangle and another.
    ///
    /// Returns `None` if the two rectangles don't overlap.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let (x, width) = line_intersect(self.x, self.width, other.x, other.width)?;
        let (y, height) = line_intersect(self.y, self.height, other.y, other.height)?;

        Some(Rect {
            x,
            y,
            width,
            height,
        })
    }
}

fn line_intersect(base: u32, len: u32, other_base: u32, other_len: u32) -> Option<(u32, u32)> {
    if base < other_base {
        let overlap_len = len.checked_sub(other_base - base)?;
        Some((other_base, cmp::min(overlap_len, other_len)))
    } else {
        let overlap_len = other_len.checked_sub(base - other_base)?;
        Some((base, cmp::min(overlap_len, len)))
    }
}
