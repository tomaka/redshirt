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

/// Support for VBE 2.0 and VGA.
mod vbe;

fn main() {
    redshirt_log_interface::init();
    redshirt_syscalls::block_on(async_main());
}

async fn async_main() {
    let mut vbe = vbe::VbeContext::new().await;
    vbe.set_ax(0x4f00);
    vbe.set_es_di(0x50, 0x0);
    vbe.write_memory(0x500, &b"VBE2"[..]);
    vbe.int10h();
    assert_eq!(vbe.ax(), 0x4f);

    let mut info_out = [0; 512];
    vbe.read_memory(0x500, &mut info_out[..]);
    assert_eq!(&info_out[0..4], b"VESA");

    let video_modes = {
        let vmodes_seg = vbe.read_memory_u16(0x510);
        let vmodes_ptr = vbe.read_memory_u16(0x50e);
        let mut vmodes_addr = (u32::from(vmodes_seg) << 4) + u32::from(vmodes_ptr);
        let mut modes = Vec::new();
        loop {
            let mode = vbe.read_memory_u16(vmodes_addr);
            if mode == 0xffff {
                break modes;
            }
            vmodes_addr += 2;
            modes.push(mode);
        }
    };
    log::info!("Video modes = {:?}", video_modes);

    let total_memory = u32::from(vbe.read_memory_u16(0x512)) * 64 * 1024;
    log::info!("Total memory = 0x{:x}", total_memory);

    let oem_string = {
        let seg = vbe.read_memory_u16(0x508);
        let ptr = vbe.read_memory_u16(0x506);
        let addr = (u32::from(seg) << 4) + u32::from(ptr);
        vbe.read_memory_nul_terminated_str(addr)
    };
    log::info!("OEM string: {}", oem_string);
}
