// Copyright(c) 2019 Pierre Krieger

use futures::prelude::*;

pub async fn get(_hash: &[u8; 32]) -> impl AsyncRead {
    // TODO: duh
    std::io::Cursor::new(&include_bytes!("../../preloaded/target/wasm32-unknown-unknown/release/preloaded.wasm")[..])
}
