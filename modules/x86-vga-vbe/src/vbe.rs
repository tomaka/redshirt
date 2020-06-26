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

use crate::interpreter;
use core::{convert::TryFrom as _, fmt};

/// Access to VBE functions.
pub struct VbeContext {
    interpreter: interpreter::Interpreter,
    video_modes: Vec<ModeInfo>,
}

/// Try to fetch information about the supported video modes from the hardware.
///
/// # Safety
///
/// Must only ever be called once at a time. No other code should access the VGA BIOS or the video
/// card while this call is in progress or the returned [`VbeContext`] is in use.
///
pub async unsafe fn load_vbe_info() -> Result<VbeContext, Error> {
    let mut interpreter = interpreter::Interpreter::from_real_machine().await;

    // We start by asking the BIOS for general information about the graphics device.
    interpreter.set_ax(0x4f00);
    interpreter.set_es_di(0x50, 0x0); // Fill 512 bytes at address 0x500.
    interpreter.write_memory(0x500, &b"VBE2"[..]);
    interpreter.int10h()?;
    check_ax(interpreter.ax())?;

    // Read out what the BIOS has written.
    let mut info_out = [0; 512];
    interpreter.read_memory(0x500, &mut info_out[..]);
    if &info_out[0..4] != b"VESA" {
        return Err(Error::BadMagic);
    }

    // Note that there's a bunch of information we can read from this data, but apart from the
    // video mode we don't actually care about any of them. For instance, why is the amount of
    // memory on the video card even reported, other than for showing fancy statistics to the
    // user?

    // The data structure contains a far pointer to the list of video modes. We now retreive the
    // content of this list.
    let video_modes_nums: Vec<u16> = {
        let vmodes_seg = interpreter.read_memory_u16(0x510);
        let vmodes_ptr = interpreter.read_memory_u16(0x50e);
        let mut vmodes_addr = (u32::from(vmodes_seg) << 4) + u32::from(vmodes_ptr);
        let mut modes = Vec::new();
        loop {
            let mode = interpreter.read_memory_u16(vmodes_addr);
            vmodes_addr += 2;
            // A `0xffff` value indicates the end of the list of video modes.
            if mode == 0xffff {
                break modes;
            }
            // Modes are defined to always be only 9bits long.
            if mode >= (1 << 9) {
                log::warn!("Skipping invalid video mode number: 0x{:x}", mode);
                continue;
            }
            modes.push(mode);
        }
    };

    // For each reported mode, perform an int10h call to retreive information about it and fill
    // the `video_modes` list.
    let mut video_modes = Vec::<ModeInfo>::with_capacity(video_modes_nums.len());
    for mode in video_modes_nums {
        interpreter.set_ax(0x4f01);
        interpreter.set_cx(mode);
        // TODO: better location
        interpreter.set_es_di(0x50, 0x0);

        // Try to call the VBE function, but ignored that specific mode if the call fails.
        if let Err(err) = interpreter.int10h() {
            continue;
        }
        if check_ax(interpreter.ax()).is_err() {
            continue;
        }

        // The VBE function wrote the information in memory.
        let mut info_out = [0; 256];
        interpreter.read_memory(0x500, &mut info_out[..]);

        let mode_attributes = u16::from_le_bytes(<[u8; 2]>::try_from(&info_out[0..2]).unwrap());
        if mode_attributes & (1 << 0) == 0 {
            // Skip unsupported video modes.
            continue;
        }
        if mode_attributes & (1 << 4) == 0 {
            // Skip text modes.
            continue;
        }
        if mode_attributes & (1 << 7) == 0 {
            // Skip modes that don't support the linear framebuffer.
            continue;
        }

        let x_resolution = u16::from_le_bytes(<[u8; 2]>::try_from(&info_out[0x12..0x14]).unwrap());
        let y_resolution = u16::from_le_bytes(<[u8; 2]>::try_from(&info_out[0x14..0x16]).unwrap());
        let phys_base = u32::from_le_bytes(<[u8; 4]>::try_from(&info_out[0x28..0x2c]).unwrap());

        video_modes.push(ModeInfo {
            mode_num: mode,
            x_resolution,
            y_resolution,
            phys_base: u64::from(phys_base),
        });
    }

    Ok(VbeContext {
        interpreter,
        video_modes,
    })
}

impl VbeContext {
    /// Returns the list of available modes.
    pub fn modes<'a>(&'a self) -> impl ExactSizeIterator<Item = Mode<'a>> + 'a {
        self.video_modes.iter().map(|info| Mode { info })
    }

    /// Try to switch to the given video mode.
    ///
    /// > **Note**: As documented in [`load_vbe_info`], no other code should access the video card
    /// >           or the VGA BIOS while this call is in progress.
    ///
    /// # Panic
    ///
    /// The mode number must be one of the supported modes.
    ///
    pub async fn set_current_mode(&mut self, mode_num: u16) -> Result<(), Error> {
        assert!(self.video_modes.iter().any(|m| m.mode_num == mode_num));

        self.interpreter.set_ax(0x4f02);

        // Bit 14 requests to use the linear framebuffer.
        // Note that bit 15 can normally be set in order to ask the BIOS to clear the screen,
        // but we don't expose this feature as the specifications mention that it is not
        // actually mandatory for the BIOS to do so.
        self.interpreter.set_bx((1 << 14) | mode_num);

        // Note that in case of failure such as an unsupported opcode, we might be in the middle
        // of a mode switch and the video card might be in an inconsistent state. Unfortunately,
        // there is no way to ask the video card to revert to the previous mode.
        //
        // While switching back to the previous mode seems like a legitimate idea, there are two
        // major obstacles to this:
        //
        // - The VBE specs don't give a way to know what the initial mode is. The "return current
        //   VBE mode" (0x3) function is only guaranteed to give a meaningful result if the mode
        //   wasn't set using the "set current mode" function.
        // - Chances are high that an error that happens when switching mode is a bug that would
        //   happen as well when switching back to the previous.
        //

        self.interpreter.int10h()?;
        check_ax(self.interpreter.ax())?;

        Ok(())
    }
}

/// Access to a single mode within the [`VbeContext`].
#[derive(Debug)]
pub struct Mode<'a> {
    info: &'a ModeInfo,
}

#[derive(Debug)]
struct ModeInfo {
    /// Identifier for this mode.
    mode_num: u16,
    /// Number of pixels for the width.
    x_resolution: u16,
    /// Number of pixels for the height.
    y_resolution: u16,
    /// Base for the physical address of a linear framebuffer for this mode.
    phys_base: u64,
}

impl<'a> Mode<'a> {
    /// Returns the mode number, to pass to [`VbeContext::set_current_mode`].
    pub fn num(&self) -> u16 {
        self.info.mode_num
    }

    /// Get the number of pixels for respectively the width and height.
    pub fn pixels_dimensions(&self) -> (u16, u16) {
        (self.info.x_resolution, self.info.y_resolution)
    }

    /// Returns the physical memory location of the linear framebuffer once we will have switched
    /// to this mode.
    pub fn linear_framebuffer_location(&self) -> u64 {
        self.info.phys_base
    }
}

/// Error while calling VBE functioons.
#[derive(Debug, derive_more::Display, derive_more::From)]
pub enum Error {
    /// VBE implementation doesn't support the requested function.
    #[display(fmt = "VBE implementation doesn't support requested function")]
    #[from(ignore)]
    NotSupported,
    /// VBE implementation supports the function, but the call has failed for an undeterminate
    /// reason.
    #[display(fmt = "VBE function call failed. Return code: {}", ax_value)]
    #[from(ignore)]
    FunctionCallFailed {
        /// Value returned by the VBE function in the `ax` register.
        ax_value: u16,
    },
    /// Magic number produced by the VBE implementation is invalid.
    #[display(fmt = "Magic number produced by the VBE implementation is invalid")]
    #[from(ignore)]
    BadMagic,
    /// An error happened in the real mode interpreter. This indicates either a bug in the VGA
    /// BIOS or, more likely, a bug in the interpreter itself.
    #[display(fmt = "Error in i386 real mode interpreter: {}", _0)]
    InterpretationError(interpreter::Error),
}

/// Check whether the value of `ax` at the end of a VBE function call indicates that the call was
/// successful.
fn check_ax(ax: u16) -> Result<(), Error> {
    if ax == 0x4f {
        return Ok(())
    }

    if (ax & 0xf) != 0x4f {
        return Err(Error::NotSupported);
    }

    Err(Error::FunctionCallFailed {
        ax_value: ax,
    })
}
