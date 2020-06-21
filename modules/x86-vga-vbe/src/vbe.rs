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

// TODO: safety of everything in this module? things must only be called once at a time

pub struct VbeContext {
    current_mode: u16,
    interpreter: interpreter::Interpreter,
    video_modes: Vec<ModeInfo>,
}

pub struct Mode<'a> {
    info: &'a ModeInfo,
}

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

#[derive(Debug)]
pub enum Error {
    NotSupported,
    InterpretationError(interpreter::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::NotSupported => write!(f, "Function not supported"),
            Error::InterpretationError(err) => write!(f, "{}", err),
        }
    }
}

impl From<interpreter::Error> for Error {
    fn from(err: interpreter::Error) -> Error {
        Error::InterpretationError(err)
    }
}

/// Try to fetch information about the supported video modes from the hardware.
pub async fn load_vbe_info() -> Result<VbeContext, Error> {
    let mut interpreter = interpreter::Interpreter::new().await;

    // We start by asking the BIOS for general information about the graphics device.
    interpreter.set_ax(0x4f00);
    interpreter.set_es_di(0x50, 0x0); // Fill 512 bytes at address 0x500.
    interpreter.write_memory(0x500, &b"VBE2"[..]);
    interpreter.int10h()?;
    if interpreter.ax() != 0x4f {
        panic!("AX = 0x{:x}", interpreter.ax()); // TODO: debug, remove this or expand the error type
        return Err(Error::NotSupported);
    }

    // Read out what the BIOS has written.
    let mut info_out = [0; 512];
    interpreter.read_memory(0x500, &mut info_out[..]);
    if &info_out[0..4] != b"VESA" {
        return Err(Error::NotSupported);
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
        interpreter.set_es_di(0x50, 0x0);
        if let Err(err) = interpreter.int10h() {
            log::warn!(
                "Failed to call VBE function 1 for mode 0x{:x}: {}",
                mode,
                err
            );
            continue;
        }
        if interpreter.ax() != 0x4f {
            log::warn!(
                "Failed to call VBE function 1 for mode 0x{:x}: return value = 0x{:x}",
                mode,
                interpreter.ax()
            );
            continue;
        }

        let mut info_out = [0; 256];
        interpreter.read_memory(0x500, &mut info_out[..]);

        let mode_attributes = u16::from_le_bytes(<[u8; 2]>::try_from(&info_out[0..2]).unwrap());
        if mode_attributes & (1 << 0) == 0 {
            // Skip unsupported video modes.
            log::debug!("Skipping unsupported video mode 0x{:x}", mode);
            continue;
        }
        if mode_attributes & (1 << 4) == 0 {
            // Skip text modes.
            log::debug!("Skipping textual video mode 0x{:x}", mode);
            continue;
        }
        if mode_attributes & (1 << 7) == 0 {
            // Skip modes that don't support the linear framebuffer.
            log::debug!(
                "Skipping video mode 0x{:x} without a linear framebuffer",
                mode
            );
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
        current_mode: 0, // FIXME:
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
    /// # Panic
    ///
    /// The mode number must be one of the supported modes.
    pub async fn set_current_mode(&mut self, mode: u16) -> Result<(), Error> {
        assert!(self.video_modes.iter().any(|m| m.mode_num == mode));

        async fn try_switch_mode(ctxt: &mut VbeContext, mode: u16) -> Result<(), Error> {
            ctxt.interpreter.set_ax(0x4f02);
            // Bit 14 requests to use .
            // Note that bit 15 can normally be set in order to ask the BIOS to clear the screen,
            // but we don't expose this feature as the specifications mention that it is not
            // actually mandatory for the BIOS to do so.
            ctxt.interpreter.set_bx((1 << 14) | mode);
            ctxt.interpreter.int10h()?;
            if ctxt.interpreter.ax() != 0x4f {
                return Err(Error::NotSupported);
            }
            Ok(())
        }

        if let Err(err) = try_switch_mode(self, mode).await {
            // If an error happened while switching mode, we might be in the middle of the mode
            // switch in some sort of inconsistent state. Try to revert back to what we had.
            let _ = try_switch_mode(self, self.current_mode).await;
            return Err(err);
        }

        self.current_mode = mode;
        Ok(())
    }
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
