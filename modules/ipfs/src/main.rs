// Copyright(c) 2019 Pierre Krieger

fn main() {
    //syscalls::register_interface(&[0; 32]).unwrap();
    tcp::TcpStream::connect(&"127.0.0.1:8000".parse().unwrap());
}
