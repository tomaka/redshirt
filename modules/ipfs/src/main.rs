// Copyright(c) 2019 Pierre Krieger

fn main() {
    syscalls::register_interface("loader", foo);
    tcp::TcpStream::connect(&"127.0.0.1:8000".parse().unwrap());
}

extern fn foo() {

}
