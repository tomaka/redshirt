// Copyright(c) 2019 Pierre Krieger

use byteorder::{ByteOrder as _, LittleEndian};
use std::io::Write as _;

pub fn fd_write(
    system: &mut kernel_core::system::System<impl Clone>,
    pid: kernel_core::scheduler::Pid,
    thread_id: kernel_core::scheduler::ThreadId,
    params: Vec<wasmi::RuntimeValue>
) {
    assert_eq!(params.len(), 4);
    //println!("{:?}", params);
    //assert!(params[0] == wasmi::RuntimeValue::I32(0) || params[0] == wasmi::RuntimeValue::I32(1));      // either stdout or stderr
    let addr = params[1].try_into::<i32>().unwrap() as usize;
    assert_eq!(params[2], wasmi::RuntimeValue::I32(1));
    let mem = system.read_memory(pid, addr..addr + 4).unwrap();
    let mem = ((mem[0] as u32)
        | ((mem[1] as u32) << 8)
        | ((mem[2] as u32) << 16)
        | ((mem[3] as u32) << 24)) as usize;
    let buf_size = system.read_memory(pid, addr + 4..addr + 8).unwrap();
    let buf_size = ((buf_size[0] as u32)
        | ((buf_size[1] as u32) << 8)
        | ((buf_size[2] as u32) << 16)
        | ((buf_size[3] as u32) << 24))
        as usize;
    let buf = system.read_memory(pid, mem..mem + buf_size).unwrap();
    //std::io::stdout().write_all(b"Message from process: ").unwrap();
    std::io::stdout().write_all(&buf).unwrap();
    //std::io::stdout().write_all(b"\r").unwrap();
    std::io::stdout().flush().unwrap();

    let out_ptr = params[3].try_into::<i32>().unwrap() as u32;
    let mut buf = [0; 4];
    LittleEndian::write_u32(&mut buf, buf_size as u32);
    system.write_memory(pid, out_ptr, &buf).unwrap();

    system.resolve_extrinsic_call(
        thread_id,
        Some(wasmi::RuntimeValue::I32(0)),
    );
}
