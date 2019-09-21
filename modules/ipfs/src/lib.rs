// Copyright(c) 2019 Pierre Krieger

//mod tcp_transport;

use futures::prelude::*;

pub async fn get(_hash: &[u8; 32]) -> impl AsyncRead {
    // TODO: duh
    std::io::Cursor::new(
        &include_bytes!("../../../target/wasm32-unknown-unknown/release/preloaded.wasm")[..],
    )
}
