// Copyright(c) 2019 Pierre Krieger

use byteorder::{ByteOrder as _, LittleEndian};
use std::io::Write as _;

pub fn fd_write(
    system: &mut kernel_core::system::System<impl Clone>,
    pid: kernel_core::scheduler::Pid,
    thread_id: kernel_core::scheduler::ThreadId,
    params: Vec<wasmi::RuntimeValue>
) {
    assert_eq!(params.len(), 4);        // TODO: what to do when it's not the case?

    //assert!(params[0] == wasmi::RuntimeValue::I32(1) || params[0] == wasmi::RuntimeValue::I32(2));      // either stdout or stderr

    // Get a list of pointers and lengths to write.
    // Elements 0, 2, 4, 6, ... or that list are pointers, and elements 1, 3, 5, 7, ... are
    // lengths.
    let list_to_write = {
        let addr = params[1].try_into::<i32>().unwrap() as usize;
        let num = params[2].try_into::<i32>().unwrap() as usize;
        let list_buf = system.read_memory(pid, addr..addr + 4 * num * 2).unwrap();
        let mut list_out = vec![0u32; num * 2];
        LittleEndian::read_u32_into(&list_buf, &mut list_out);
        list_out
    };

    let mut total_written = 0;

    for ptr_and_len in list_to_write.windows(2) {
        let ptr = ptr_and_len[0] as usize;
        let len = ptr_and_len[1] as usize;

        let to_write = system.read_memory(pid, ptr..ptr + len).unwrap();
        std::io::stdout().write_all(&to_write).unwrap();
        total_written += to_write.len();
    }

    // Write to the fourth parameter the number of bytes written to the file descriptor.
    {
        let out_ptr = params[3].try_into::<i32>().unwrap() as u32;
        let mut buf = [0; 4];
        LittleEndian::write_u32(&mut buf, total_written as u32);
        system.write_memory(pid, out_ptr, &buf).unwrap();
    }

    system.resolve_extrinsic_call(
        thread_id,
        Some(wasmi::RuntimeValue::I32(0)),
    );
}
