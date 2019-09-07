// Copyright(c) 2019 Pierre Krieger

fn main() {
    let mut core = core::core::Core::new();
    let module = core::module::Module::from_bytes(&include_bytes!("../../modules/preloaded/target/wasm32-unknown-unknown/release/preloaded.wasm")[..]);
    core.execute(&module).unwrap();
    futures::executor::block_on(core.run());
}
