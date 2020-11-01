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

#![cfg(test)]

use super::Interpreter;

#[test]
fn basic_entry_point_works() {
    futures::executor::block_on(async move {
        let mut interpreter =
            Interpreter::from_memory(include_bytes!("test-mem.bin").to_vec()).await;
        interpreter.disable_io_operations();
        interpreter.set_ax(0x4f00);
        interpreter.set_es_di(0x50, 0x0);
        interpreter.write_memory(0x500, &b"VBE2"[..]);
        interpreter.int10h().unwrap();
        assert_eq!(interpreter.ax(), 0x4f);

        let mut info_out = [0; 512];
        interpreter.read_memory(0x500, &mut info_out[..]);
        assert_eq!(&info_out[0..4], b"VESA");

        let video_modes = {
            let vmodes_seg = interpreter.read_memory_u16(0x510);
            let vmodes_ptr = interpreter.read_memory_u16(0x50e);
            let mut vmodes_addr = (u32::from(vmodes_seg) << 4) + u32::from(vmodes_ptr);
            let mut modes = Vec::new();
            loop {
                let mode = interpreter.read_memory_u16(vmodes_addr);
                if mode == 0xffff {
                    break modes;
                }
                vmodes_addr += 2;
                modes.push(mode);
            }
        };
        log::info!("Video modes = {:?}", video_modes);

        let total_memory = u32::from(interpreter.read_memory_u16(0x512)) * 64 * 1024;
        log::info!("Total memory = 0x{:x}", total_memory);

        let oem_string = {
            let seg = interpreter.read_memory_u16(0x508);
            let ptr = interpreter.read_memory_u16(0x506);
            let addr = (u32::from(seg) << 4) + u32::from(ptr);
            interpreter.read_memory_nul_terminated_str(addr)
        };
        log::info!("OEM string: {}", oem_string);

        for mode in video_modes.iter().take(1) {
            // TODO: remove take(1)
            interpreter.set_ax(0x4f01);
            interpreter.set_cx(*mode);
            interpreter.set_es_di(0x50, 0x0);
            interpreter.int10h().unwrap();
            log::error!("EAX after call: 0x{:x}", interpreter.ax());

            let mut info_out = [0; 256];
            interpreter.read_memory(0x500, &mut info_out[..]);
            log::debug!("Mode info: {:?}", &info_out[..]);
        }
    })
}
