// Copyright(c) 2019 Pierre Krieger

use futures::prelude::*;

fn main() {
    //syscalls::register_interface(&[0; 32]).unwrap();

    futures::executor::block_on(async move {
        let mut tcp_stream = tcp::TcpStream::connect(&"127.0.0.1:8000".parse().unwrap()).await;

        tcp_stream.write_all(br#"GET / HTTP/1.1
User-Agent: Mozilla/4.0 (compatible; MSIE5.01; Windows NT)
Host: localhost
Connection: Keep-Alive

"#).await.unwrap();
        tcp_stream.flush().await.unwrap();

        let mut out = vec![0; 65536];
        tcp_stream.read(&mut out).await.unwrap();
    });
}
